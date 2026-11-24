#include "mf_h264_encoder_impl.h"

#include <mferror.h>
#include <objbase.h>
#include <oleauto.h>

#include <algorithm>
#include <chrono>
#include <cstring>

#include "api/video/i420_buffer.h"
#include "modules/video_coding/include/video_codec_interface.h"
#include "modules/video_coding/include/video_error_codes.h"
#include "rtc_base/logging.h"
#include "third_party/libyuv/include/libyuv/convert.h"

namespace webrtc {

namespace {

using Microsoft::WRL::ComPtr;

bool ContainsIdrNal(const uint8_t* data, size_t size) {
  for (size_t i = 0; i + 4 <= size; ++i) {
    size_t start_len = 0;
    if (data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1) {
      start_len = 3;
    } else if (i + 5 <= size && data[i] == 0 && data[i + 1] == 0 &&
               data[i + 2] == 0 && data[i + 3] == 1) {
      start_len = 4;
    }
    if (start_len > 0) {
      uint8_t nal_type = data[i + start_len] & 0x1F;
      if (nal_type == 5) {
        return true;
      }
      i += start_len;
    }
  }
  return false;
}

}  // namespace

MFH264EncoderImpl::MFH264EncoderImpl(const Environment& env,
                                     const SdpVideoFormat& format)
    : env_(env),
      format_(format),
      packetization_mode_(
          H264EncoderSettings::Parse(format).packetization_mode) {}

MFH264EncoderImpl::~MFH264EncoderImpl() {
  Release();
}

void MFH264EncoderImpl::SetCodecUint32(const GUID& key, uint32_t value) {
  if (!codec_api_) {
    return;
  }
  VARIANT var;
  VariantInit(&var);
  var.vt = VT_UI4;
  var.ulVal = value;
  codec_api_->SetValue(&key, &var);
  VariantClear(&var);
}

void MFH264EncoderImpl::SetCodecBool(const GUID& key, bool value) {
  if (!codec_api_) {
    return;
  }
  VARIANT var;
  VariantInit(&var);
  var.vt = VT_BOOL;
  var.boolVal = value ? VARIANT_TRUE : VARIANT_FALSE;
  codec_api_->SetValue(&key, &var);
  VariantClear(&var);
}

bool MFH264EncoderImpl::ConfigureMft(uint32_t width,
                                     uint32_t height,
                                     uint32_t fps,
                                     uint32_t bitrate_bps) {
  MFT_REGISTER_TYPE_INFO output_filter = {MFMediaType_Video,
                                          MFVideoFormat_H264};
  IMFActivate** activates = nullptr;
  UINT32 count = 0;

  HRESULT hr = MFTEnumEx(
      MFT_CATEGORY_VIDEO_ENCODER,
      MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER,
      nullptr,
      &output_filter,
      &activates,
      &count);
  if (FAILED(hr) || count == 0 || !activates) {
    RTC_LOG(LS_WARNING) << "MFH264: no hardware H.264 MFT available";
    return false;
  }

  for (UINT32 i = 0; i < count && !mft_; ++i) {
    if (!activates[i]) {
      continue;
    }
    ComPtr<IMFTransform> candidate;
    if (SUCCEEDED(activates[i]->ActivateObject(IID_PPV_ARGS(&candidate))) &&
        candidate) {
      mft_ = candidate;
    }
  }
  for (UINT32 i = 0; i < count; ++i) {
    if (activates[i]) {
      activates[i]->Release();
    }
  }
  CoTaskMemFree(activates);

  if (!mft_) {
    RTC_LOG(LS_WARNING) << "MFH264: failed to activate hardware H.264 MFT";
    return false;
  }

  // Unlock async hardware MFTs (AMD AMF, Intel QSV, NVIDIA MFT).
  ComPtr<IMFAttributes> attrs;
  if (SUCCEEDED(mft_->GetAttributes(&attrs)) && attrs) {
    UINT32 async_flag = 0;
    if (SUCCEEDED(attrs->GetUINT32(MF_TRANSFORM_ASYNC, &async_flag)) &&
        async_flag != 0) {
      hr = attrs->SetUINT32(MF_TRANSFORM_ASYNC_UNLOCK, TRUE);
      if (FAILED(hr)) {
        RTC_LOG(LS_WARNING) << "MFH264: failed to unlock async MFT";
        return false;
      }
      hr = mft_.As(&event_gen_);
      if (FAILED(hr) || !event_gen_) {
        RTC_LOG(LS_WARNING) << "MFH264: async MFT missing IMFMediaEventGenerator";
        return false;
      }
      is_async_ = true;
    }
  }

  DWORD in_count = 0;
  DWORD out_count = 0;
  if (SUCCEEDED(mft_->GetStreamCount(&in_count, &out_count)) &&
      in_count > 0 && out_count > 0) {
    std::vector<DWORD> in_ids(in_count, 0);
    std::vector<DWORD> out_ids(out_count, 0);
    if (SUCCEEDED(
            mft_->GetStreamIDs(in_count, in_ids.data(), out_count, out_ids.data()))) {
      in_stream_id_ = in_ids[0];
      out_stream_id_ = out_ids[0];
    } else {
      in_stream_id_ = 0;
      out_stream_id_ = 0;
    }
  }

  // Configure low-latency WebRTC properties before media types.
  mft_.As(&codec_api_);
  SetCodecBool(CODECAPI_AVLowLatencyMode, true);
  SetCodecUint32(CODECAPI_AVEncCommonRateControlMode,
                 eAVEncCommonRateControlMode_CBR);
  SetCodecUint32(CODECAPI_AVEncCommonMeanBitRate, bitrate_bps);
  SetCodecUint32(CODECAPI_AVEncCommonQualityVsSpeed, 33);
  SetCodecBool(CODECAPI_AVEncH264CABACEnable, false);
  SetCodecUint32(CODECAPI_AVEncMPVDefaultBPictureCount, 0);
  SetCodecUint32(CODECAPI_AVEncMPVGOPSize, std::max<uint32_t>(fps * 5, 30));

  ComPtr<IMFMediaType> out_type;
  hr = MFCreateMediaType(&out_type);
  if (FAILED(hr)) {
    return false;
  }
  out_type->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Video);
  out_type->SetGUID(MF_MT_SUBTYPE, MFVideoFormat_H264);
  out_type->SetUINT32(MF_MT_AVG_BITRATE, bitrate_bps);
  out_type->SetUINT32(MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive);
  out_type->SetUINT32(MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_Base);
  // Level 3.1 covers 720p30. Anything larger is level 4.0, matching the
  // 42e028 offer so the SPS does not exceed the negotiated level.
  const uint32_t h264_level =
      (height > 720 || fps > 30) ? eAVEncH264VLevel4 : eAVEncH264VLevel3_1;
  out_type->SetUINT32(MF_MT_MPEG2_LEVEL, h264_level);
  SetCodecUint32(CODECAPI_AVEncMPVLevel, h264_level);
  MFSetAttributeSize(out_type.Get(), MF_MT_FRAME_SIZE, width, height);
  MFSetAttributeRatio(out_type.Get(), MF_MT_FRAME_RATE, fps, 1);
  MFSetAttributeRatio(out_type.Get(), MF_MT_PIXEL_ASPECT_RATIO, 1, 1);

  hr = mft_->SetOutputType(out_stream_id_, out_type.Get(), 0);
  if (FAILED(hr)) {
    RTC_LOG(LS_WARNING) << "MFH264: SetOutputType failed: 0x" << std::hex << hr;
    return false;
  }

  ComPtr<IMFMediaType> in_type;
  hr = MFCreateMediaType(&in_type);
  if (FAILED(hr)) {
    return false;
  }
  in_type->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Video);
  in_type->SetGUID(MF_MT_SUBTYPE, MFVideoFormat_NV12);
  in_type->SetUINT32(MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive);
  MFSetAttributeSize(in_type.Get(), MF_MT_FRAME_SIZE, width, height);
  MFSetAttributeRatio(in_type.Get(), MF_MT_FRAME_RATE, fps, 1);
  MFSetAttributeRatio(in_type.Get(), MF_MT_PIXEL_ASPECT_RATIO, 1, 1);

  hr = mft_->SetInputType(in_stream_id_, in_type.Get(), 0);
  if (FAILED(hr)) {
    RTC_LOG(LS_WARNING) << "MFH264: SetInputType(NV12) failed: 0x" << std::hex
                        << hr;
    return false;
  }

  MFT_OUTPUT_STREAM_INFO out_info = {};
  if (SUCCEEDED(mft_->GetOutputStreamInfo(out_stream_id_, &out_info))) {
    output_provides_samples_ =
        (out_info.dwFlags &
         (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES |
          MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES)) != 0;
  }

  hr = mft_->ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0);
  if (FAILED(hr)) {
    RTC_LOG(LS_WARNING) << "MFH264: BEGIN_STREAMING failed: 0x" << std::hex
                        << hr;
    return false;
  }
  hr = mft_->ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0);
  if (FAILED(hr)) {
    RTC_LOG(LS_WARNING) << "MFH264: START_OF_STREAM failed: 0x" << std::hex
                        << hr;
    return false;
  }

  RTC_LOG(LS_INFO) << "MFH264: initialized " << width << "x" << height << " @"
                   << fps << "fps " << (bitrate_bps / 1000) << " kbps (async="
                   << (is_async_ ? "true" : "false") << ")";
  return true;
}

int32_t MFH264EncoderImpl::InitEncode(const VideoCodec* inst,
                                      const Settings& /*settings*/) {
  if (!inst || inst->codecType != kVideoCodecH264 || inst->width < 2 ||
      inst->height < 2) {
    return WEBRTC_VIDEO_CODEC_ERR_PARAMETER;
  }

  Release();

  HRESULT hr_com = CoInitializeEx(nullptr, COINIT_MULTITHREADED);
  com_initialized_ = SUCCEEDED(hr_com);

  if (FAILED(MFStartup(MF_VERSION, MFSTARTUP_LITE))) {
    Release();
    return WEBRTC_VIDEO_CODEC_FALLBACK_SOFTWARE;
  }
  mf_started_ = true;

  width_ = inst->width;
  height_ = inst->height;
  max_fps_ = std::max<uint32_t>(inst->maxFramerate, 1);
  target_bps_ = std::max<uint32_t>(inst->startBitrate * 1000, 150000);
  frame_index_ = 0;
  sending_ = true;
  key_frame_request_ = true;
  async_need_input_ = 0;
  pending_meta_.clear();

  if (!ConfigureMft(width_, height_, max_fps_, target_bps_)) {
    Release();
    return WEBRTC_VIDEO_CODEC_FALLBACK_SOFTWARE;
  }

  initialized_ = true;
  return WEBRTC_VIDEO_CODEC_OK;
}

int32_t MFH264EncoderImpl::RegisterEncodeCompleteCallback(
    EncodedImageCallback* callback) {
  encoded_image_callback_ = callback;
  return WEBRTC_VIDEO_CODEC_OK;
}

int32_t MFH264EncoderImpl::Release() {
  if (mft_) {
    mft_->ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0);
    mft_->ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0);
  }
  event_gen_.Reset();
  codec_api_.Reset();
  mft_.Reset();
  pending_meta_.clear();
  initialized_ = false;
  is_async_ = false;
  async_need_input_ = 0;

  if (mf_started_) {
    MFShutdown();
    mf_started_ = false;
  }
  if (com_initialized_) {
    CoUninitialize();
    com_initialized_ = false;
  }
  return WEBRTC_VIDEO_CODEC_OK;
}

void MFH264EncoderImpl::EmitEncodedBuffer(const uint8_t* data,
                                          size_t size,
                                          bool clean_point,
                                          LONGLONG sample_time) {
  if (!encoded_image_callback_ || !data || size == 0) {
    return;
  }

  PendingFrameMeta meta = {0, 0, std::nullopt};
  auto matched = pending_meta_.find(sample_time);
  if (matched != pending_meta_.end()) {
    meta = matched->second;
  }
  // Drop this sample and any older unmatched input. A dropped MFT frame must
  // not shift the RTP timestamp of every later frame.
  if (sample_time >= 0) {
    auto end = pending_meta_.upper_bound(sample_time);
    pending_meta_.erase(pending_meta_.begin(), end);
  }

  const bool is_key = clean_point || ContainsIdrNal(data, size);

  encoded_image_.SetEncodedData(EncodedImageBuffer::Create(data, size));
  h264_bitstream_parser_.ParseBitstream(encoded_image_);
  encoded_image_.qp_ = h264_bitstream_parser_.GetLastSliceQp().value_or(-1);
  encoded_image_._encodedWidth = width_;
  encoded_image_._encodedHeight = height_;
  encoded_image_.SetRtpTimestamp(meta.rtp_timestamp);
  encoded_image_.capture_time_ms_ = meta.capture_time_ms;
  encoded_image_.SetColorSpace(meta.color_space);
  encoded_image_._frameType =
      is_key ? VideoFrameType::kVideoFrameKey : VideoFrameType::kVideoFrameDelta;

  CodecSpecificInfo codec_specific;
  codec_specific.codecType = kVideoCodecH264;
  codec_specific.codecSpecific.H264.packetization_mode = packetization_mode_;
  codec_specific.codecSpecific.H264.temporal_idx = kNoTemporalIdx;
  codec_specific.codecSpecific.H264.base_layer_sync = false;
  codec_specific.codecSpecific.H264.idr_frame = is_key;

  encoded_image_callback_->OnEncodedImage(encoded_image_, &codec_specific);
}

bool MFH264EncoderImpl::ProcessOneOutputSample() {
  MFT_OUTPUT_DATA_BUFFER out_buf = {};
  out_buf.dwStreamID = out_stream_id_;
  ComPtr<IMFSample> owned_sample;

  if (!output_provides_samples_) {
    MFT_OUTPUT_STREAM_INFO info = {};
    if (FAILED(mft_->GetOutputStreamInfo(out_stream_id_, &info))) {
      return false;
    }
    DWORD alloc_size = std::max<DWORD>(info.cbSize, width_ * height_ * 3 / 2);
    ComPtr<IMFMediaBuffer> buf;
    if (FAILED(MFCreateMemoryBuffer(alloc_size, &buf)) ||
        FAILED(MFCreateSample(&owned_sample))) {
      return false;
    }
    owned_sample->AddBuffer(buf.Get());
    out_buf.pSample = owned_sample.Get();
  }

  DWORD status = 0;
  HRESULT hr = mft_->ProcessOutput(0, 1, &out_buf, &status);

  if (out_buf.pEvents) {
    out_buf.pEvents->Release();
    out_buf.pEvents = nullptr;
  }

  if (hr == MF_E_TRANSFORM_STREAM_CHANGE) {
    ComPtr<IMFMediaType> new_type;
    if (SUCCEEDED(mft_->GetOutputAvailableType(out_stream_id_, 0, &new_type))) {
      mft_->SetOutputType(out_stream_id_, new_type.Get(), 0);
    }
    return true;
  }

  if (FAILED(hr) || !out_buf.pSample) {
    return false;
  }

  ComPtr<IMFSample> sample;
  if (output_provides_samples_) {
    sample.Attach(out_buf.pSample);
  } else {
    sample = owned_sample;
  }

  UINT32 clean_point = 0;
  sample->GetUINT32(MFSampleExtension_CleanPoint, &clean_point);
  LONGLONG sample_time = -1;
  sample->GetSampleTime(&sample_time);

  ComPtr<IMFMediaBuffer> contiguous;
  if (SUCCEEDED(sample->ConvertToContiguousBuffer(&contiguous)) && contiguous) {
    BYTE* ptr = nullptr;
    DWORD cur_len = 0;
    if (SUCCEEDED(contiguous->Lock(&ptr, nullptr, &cur_len)) && ptr &&
        cur_len > 0) {
      EmitEncodedBuffer(ptr, cur_len, clean_point != 0, sample_time);
      contiguous->Unlock();
    }
  }
  return true;
}

bool MFH264EncoderImpl::DrainSyncOutput() {
  for (;;) {
    MFT_OUTPUT_DATA_BUFFER out_buf = {};
    out_buf.dwStreamID = out_stream_id_;
    ComPtr<IMFSample> owned_sample;

    if (!output_provides_samples_) {
      MFT_OUTPUT_STREAM_INFO info = {};
      if (FAILED(mft_->GetOutputStreamInfo(out_stream_id_, &info))) {
        return false;
      }
      DWORD alloc_size = std::max<DWORD>(info.cbSize, width_ * height_ * 3 / 2);
      ComPtr<IMFMediaBuffer> buf;
      if (FAILED(MFCreateMemoryBuffer(alloc_size, &buf)) ||
          FAILED(MFCreateSample(&owned_sample))) {
        return false;
      }
      owned_sample->AddBuffer(buf.Get());
      out_buf.pSample = owned_sample.Get();
    }

    DWORD status = 0;
    HRESULT hr = mft_->ProcessOutput(0, 1, &out_buf, &status);
    if (out_buf.pEvents) {
      out_buf.pEvents->Release();
      out_buf.pEvents = nullptr;
    }
    if (hr == MF_E_TRANSFORM_NEED_MORE_INPUT) {
      return true;
    }
    if (hr == MF_E_TRANSFORM_STREAM_CHANGE) {
      ComPtr<IMFMediaType> new_type;
      if (SUCCEEDED(
              mft_->GetOutputAvailableType(out_stream_id_, 0, &new_type))) {
        mft_->SetOutputType(out_stream_id_, new_type.Get(), 0);
      }
      continue;
    }
    if (FAILED(hr) || !out_buf.pSample) {
      return false;
    }

    ComPtr<IMFSample> sample;
    if (output_provides_samples_) {
      sample.Attach(out_buf.pSample);
    } else {
      sample = owned_sample;
    }

    UINT32 clean_point = 0;
    sample->GetUINT32(MFSampleExtension_CleanPoint, &clean_point);
    LONGLONG sample_time = -1;
    sample->GetSampleTime(&sample_time);

    ComPtr<IMFMediaBuffer> contiguous;
    if (SUCCEEDED(sample->ConvertToContiguousBuffer(&contiguous)) &&
        contiguous) {
      BYTE* ptr = nullptr;
      DWORD cur_len = 0;
      if (SUCCEEDED(contiguous->Lock(&ptr, nullptr, &cur_len)) && ptr &&
          cur_len > 0) {
        EmitEncodedBuffer(ptr, cur_len, clean_point != 0, sample_time);
        contiguous->Unlock();
      }
    }
  }
}

bool MFH264EncoderImpl::PumpAsyncUntilInputNeeded() {
  const auto deadline =
      std::chrono::steady_clock::now() + std::chrono::milliseconds(200);
  while (async_need_input_ <= 0) {
    if (std::chrono::steady_clock::now() >= deadline) {
      RTC_LOG(LS_WARNING)
          << "MFH264: async MFT sent no input event within 200 ms";
      return false;
    }
    ComPtr<IMFMediaEvent> event;
    HRESULT hr = event_gen_->GetEvent(MF_EVENT_FLAG_NO_WAIT, &event);
    if (hr == MF_E_NO_EVENTS_AVAILABLE) {
      Sleep(1);
      continue;
    }
    if (FAILED(hr) || !event) {
      return false;
    }
    MediaEventType type = MEUnknown;
    event->GetType(&type);
    if (type == METransformNeedInput) {
      ++async_need_input_;
    } else if (type == METransformHaveOutput) {
      if (!ProcessOneOutputSample()) {
        return false;
      }
    }
  }
  return async_need_input_ > 0;
}

bool MFH264EncoderImpl::DrainAsyncReadyOutputs() {
  for (int attempts = 0; attempts < 16; ++attempts) {
    ComPtr<IMFMediaEvent> event;
    HRESULT hr = event_gen_->GetEvent(MF_EVENT_FLAG_NO_WAIT, &event);
    if (hr == MF_E_NO_EVENTS_AVAILABLE) {
      return true;
    }
    if (FAILED(hr) || !event) {
      return false;
    }
    MediaEventType type = MEUnknown;
    event->GetType(&type);
    if (type == METransformNeedInput) {
      ++async_need_input_;
    } else if (type == METransformHaveOutput) {
      if (!ProcessOneOutputSample()) {
        return false;
      }
    }
  }
  return true;
}

int32_t MFH264EncoderImpl::Encode(
    const VideoFrame& input_frame,
    const std::vector<VideoFrameType>* frame_types) {
  if (!initialized_ || !mft_ || !encoded_image_callback_) {
    return WEBRTC_VIDEO_CODEC_UNINITIALIZED;
  }
  if (!sending_) {
    return WEBRTC_VIDEO_CODEC_NO_OUTPUT;
  }
  if (frame_types && !frame_types->empty() &&
      (*frame_types)[0] == VideoFrameType::kEmptyFrame) {
    return WEBRTC_VIDEO_CODEC_NO_OUTPUT;
  }

  webrtc::scoped_refptr<I420BufferInterface> i420 =
      input_frame.video_frame_buffer()->ToI420();
  if (!i420) {
    return WEBRTC_VIDEO_CODEC_ENCODER_FAILURE;
  }

  if (static_cast<uint32_t>(i420->width()) != width_ ||
      static_cast<uint32_t>(i420->height()) != height_) {
    // Resolution changed dynamically: re-init MFT or fall back cleanly.
    width_ = static_cast<uint32_t>(i420->width());
    height_ = static_cast<uint32_t>(i420->height());
    mft_.Reset();
    codec_api_.Reset();
    event_gen_.Reset();
    is_async_ = false;
    async_need_input_ = 0;
    pending_meta_.clear();
    if (!ConfigureMft(width_, height_, max_fps_, target_bps_)) {
      return WEBRTC_VIDEO_CODEC_FALLBACK_SOFTWARE;
    }
    key_frame_request_ = true;
  }

  const bool send_key_frame =
      key_frame_request_ ||
      (frame_types && !frame_types->empty() &&
       (*frame_types)[0] == VideoFrameType::kVideoFrameKey);
  if (send_key_frame) {
    key_frame_request_ = false;
    SetCodecUint32(CODECAPI_AVEncVideoForceKeyFrame, 1);
  }

  const uint32_t nv12_size = width_ * height_ * 3 / 2;
  ComPtr<IMFMediaBuffer> in_buffer;
  ComPtr<IMFSample> in_sample;
  if (FAILED(MFCreateMemoryBuffer(nv12_size, &in_buffer)) ||
      FAILED(MFCreateSample(&in_sample))) {
    return WEBRTC_VIDEO_CODEC_ENCODER_FAILURE;
  }

  BYTE* dst = nullptr;
  if (FAILED(in_buffer->Lock(&dst, nullptr, nullptr)) || !dst) {
    return WEBRTC_VIDEO_CODEC_ENCODER_FAILURE;
  }
  uint8_t* dst_y = dst;
  uint8_t* dst_uv = dst + width_ * height_;
  libyuv::I420ToNV12(i420->DataY(), i420->StrideY(), i420->DataU(),
                     i420->StrideU(), i420->DataV(), i420->StrideV(), dst_y,
                     static_cast<int>(width_), dst_uv,
                     static_cast<int>(width_), static_cast<int>(width_),
                     static_cast<int>(height_));
  in_buffer->Unlock();
  in_buffer->SetCurrentLength(nv12_size);
  in_sample->AddBuffer(in_buffer.Get());

  const LONGLONG duration_100ns = 10000000LL / std::max<uint32_t>(max_fps_, 1);
  const LONGLONG sample_time = frame_index_ * duration_100ns;
  in_sample->SetSampleTime(sample_time);
  in_sample->SetSampleDuration(duration_100ns);
  ++frame_index_;

  pending_meta_[sample_time] = PendingFrameMeta{
      input_frame.rtp_timestamp(),
      input_frame.render_time_ms(),
      input_frame.color_space(),
  };

  if (is_async_) {
    if (!PumpAsyncUntilInputNeeded()) {
      pending_meta_.erase(sample_time);
      return WEBRTC_VIDEO_CODEC_FALLBACK_SOFTWARE;
    }
    --async_need_input_;
    HRESULT hr = mft_->ProcessInput(in_stream_id_, in_sample.Get(), 0);
    if (FAILED(hr)) {
      pending_meta_.erase(sample_time);
      RTC_LOG(LS_WARNING) << "MFH264: async ProcessInput failed: 0x" << std::hex
                          << hr;
      return WEBRTC_VIDEO_CODEC_FALLBACK_SOFTWARE;
    }
    if (!DrainAsyncReadyOutputs()) {
      return WEBRTC_VIDEO_CODEC_FALLBACK_SOFTWARE;
    }
  } else {
    HRESULT hr = mft_->ProcessInput(in_stream_id_, in_sample.Get(), 0);
    if (FAILED(hr)) {
      pending_meta_.erase(sample_time);
      RTC_LOG(LS_WARNING) << "MFH264: sync ProcessInput failed: 0x" << std::hex
                          << hr;
      return WEBRTC_VIDEO_CODEC_FALLBACK_SOFTWARE;
    }
    if (!DrainSyncOutput()) {
      return WEBRTC_VIDEO_CODEC_FALLBACK_SOFTWARE;
    }
  }

  return WEBRTC_VIDEO_CODEC_OK;
}

void MFH264EncoderImpl::SetRates(const RateControlParameters& parameters) {
  if (!initialized_) {
    return;
  }
  if (parameters.bitrate.get_sum_bps() == 0) {
    sending_ = false;
    return;
  }
  const uint32_t new_bps =
      std::max<uint32_t>(parameters.bitrate.GetSpatialLayerSum(0), 100000);
  if (!sending_) {
    sending_ = true;
    key_frame_request_ = true;
  }
  if (parameters.framerate_fps >= 1.0) {
    max_fps_ = static_cast<uint32_t>(parameters.framerate_fps);
  }
  if (new_bps != target_bps_) {
    target_bps_ = new_bps;
    SetCodecUint32(CODECAPI_AVEncCommonMeanBitRate, target_bps_);
  }
}

VideoEncoder::EncoderInfo MFH264EncoderImpl::GetEncoderInfo() const {
  EncoderInfo info;
  info.supports_native_handle = false;
  info.implementation_name = "MediaFoundation H264";
  info.scaling_settings = VideoEncoder::ScalingSettings::kOff;
  info.is_hardware_accelerated = true;
  info.supports_simulcast = false;
  info.preferred_pixel_formats = {VideoFrameBuffer::Type::kI420};
  return info;
}

}  // namespace webrtc
