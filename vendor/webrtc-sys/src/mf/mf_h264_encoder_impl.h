#ifndef MF_H264_ENCODER_IMPL_H_
#define MF_H264_ENCODER_IMPL_H_

#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#include <codecapi.h>
#include <mfapi.h>
#include <mfidl.h>
#include <mftransform.h>
#include <strmif.h>
#include <wrl/client.h>

#include <cstdint>
#include <deque>
#include <optional>
#include <vector>

#include "api/environment/environment.h"
#include "api/video/encoded_image.h"
#include "api/video_codecs/sdp_video_format.h"
#include "api/video_codecs/video_encoder.h"
#include "common_video/h264/h264_bitstream_parser.h"
#include "modules/video_coding/codecs/h264/include/h264.h"

namespace webrtc {

class MFH264EncoderImpl : public VideoEncoder {
 public:
  MFH264EncoderImpl(const Environment& env, const SdpVideoFormat& format);
  ~MFH264EncoderImpl() override;

  int32_t InitEncode(const VideoCodec* codec_settings,
                     const Settings& settings) override;

  int32_t RegisterEncodeCompleteCallback(
      EncodedImageCallback* callback) override;

  int32_t Release() override;

  int32_t Encode(const VideoFrame& frame,
                 const std::vector<VideoFrameType>* frame_types) override;

  void SetRates(const RateControlParameters& parameters) override;

  EncoderInfo GetEncoderInfo() const override;

 private:
  struct PendingFrameMeta {
    uint32_t rtp_timestamp;
    int64_t capture_time_ms;
    std::optional<ColorSpace> color_space;
  };

  bool ConfigureMft(uint32_t width,
                    uint32_t height,
                    uint32_t fps,
                    uint32_t bitrate_bps);
  bool DrainSyncOutput();
  bool PumpAsyncUntilInputNeeded();
  bool DrainAsyncReadyOutputs();
  bool ProcessOneOutputSample();
  void EmitEncodedBuffer(const uint8_t* data,
                         size_t size,
                         bool clean_point);
  void SetCodecUint32(const GUID& key, uint32_t value);
  void SetCodecBool(const GUID& key, bool value);

  const Environment& env_;
  const SdpVideoFormat format_;
  EncodedImageCallback* encoded_image_callback_ = nullptr;
  H264PacketizationMode packetization_mode_ =
      H264PacketizationMode::NonInterleaved;
  H264BitstreamParser h264_bitstream_parser_;
  EncodedImage encoded_image_;

  bool com_initialized_ = false;
  bool mf_started_ = false;
  bool initialized_ = false;
  bool is_async_ = false;
  bool sending_ = true;
  bool key_frame_request_ = false;
  bool output_provides_samples_ = false;

  DWORD in_stream_id_ = 0;
  DWORD out_stream_id_ = 0;
  uint32_t width_ = 0;
  uint32_t height_ = 0;
  uint32_t max_fps_ = 30;
  uint32_t target_bps_ = 1000000;
  int64_t frame_index_ = 0;
  int async_need_input_ = 0;

  Microsoft::WRL::ComPtr<IMFTransform> mft_;
  Microsoft::WRL::ComPtr<ICodecAPI> codec_api_;
  Microsoft::WRL::ComPtr<IMFMediaEventGenerator> event_gen_;

  std::deque<PendingFrameMeta> pending_meta_;
};

}  // namespace webrtc

#endif  // MF_H264_ENCODER_IMPL_H_
