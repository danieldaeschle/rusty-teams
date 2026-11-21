#include "mf_encoder_factory.h"

#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#include <mfapi.h>
#include <mfidl.h>
#include <mftransform.h>
#include <objbase.h>

#include <map>
#include <memory>
#include <string>

#include "mf_h264_encoder_impl.h"
#include "rtc_base/logging.h"

namespace webrtc {

MFVideoEncoderFactory::MFVideoEncoderFactory() {
  std::map<std::string, std::string> baseline_parameters = {
      {"profile-level-id", "42e01f"},
      {"level-asymmetry-allowed", "1"},
      {"packetization-mode", "1"},
  };
  supported_formats_.push_back(SdpVideoFormat("H264", baseline_parameters));
}

MFVideoEncoderFactory::~MFVideoEncoderFactory() = default;

bool MFVideoEncoderFactory::IsSupported() {
  HRESULT hr_com = CoInitializeEx(nullptr, COINIT_MULTITHREADED);
  const bool com_owned = SUCCEEDED(hr_com);

  HRESULT hr_mf = MFStartup(MF_VERSION, MFSTARTUP_LITE);
  if (FAILED(hr_mf)) {
    if (com_owned) {
      CoUninitialize();
    }
    return false;
  }

  MFT_REGISTER_TYPE_INFO output_type = {MFMediaType_Video, MFVideoFormat_H264};
  IMFActivate** activates = nullptr;
  UINT32 count = 0;

  HRESULT hr = MFTEnumEx(
      MFT_CATEGORY_VIDEO_ENCODER,
      MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER,
      nullptr,
      &output_type,
      &activates,
      &count);

  bool supported = SUCCEEDED(hr) && count > 0;
  if (activates) {
    for (UINT32 i = 0; i < count; ++i) {
      if (activates[i]) {
        activates[i]->Release();
      }
    }
    CoTaskMemFree(activates);
  }

  MFShutdown();
  if (com_owned) {
    CoUninitialize();
  }

  if (supported) {
    RTC_LOG(LS_INFO) << "Media Foundation hardware H.264 encoder available ("
                     << count << " MFT(s))";
  } else {
    RTC_LOG(LS_INFO) << "No Media Foundation hardware H.264 encoder found";
  }
  return supported;
}

std::unique_ptr<VideoEncoder> MFVideoEncoderFactory::Create(
    const Environment& env,
    const SdpVideoFormat& format) {
  for (const auto& supported_format : supported_formats_) {
    if (format.IsSameCodec(supported_format)) {
      return std::make_unique<MFH264EncoderImpl>(env, format);
    }
  }
  return nullptr;
}

std::vector<SdpVideoFormat> MFVideoEncoderFactory::GetSupportedFormats() const {
  return supported_formats_;
}

std::vector<SdpVideoFormat> MFVideoEncoderFactory::GetImplementations() const {
  return supported_formats_;
}

}  // namespace webrtc
