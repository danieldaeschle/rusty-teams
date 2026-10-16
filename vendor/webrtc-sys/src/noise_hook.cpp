#include "livekit/noise_hook.h"

#include <string>

#include "modules/audio_processing/audio_buffer.h"
#include "rust/cxx.h"
#include "webrtc-sys/src/noise_hook.rs.h"

namespace livekit_ffi {

namespace {

class CapturePostProcessor : public webrtc::CustomProcessing {
 public:
  void Initialize(int sample_rate_hz, int num_channels) override {}

  void Process(webrtc::AudioBuffer* audio) override {
    for (size_t channel = 0; channel < audio->num_channels(); ++channel) {
      capture_post_process(
          channel,
          rust::Slice<float>(audio->channels()[channel], audio->num_frames()));
    }
  }

  std::string ToString() const override { return "livekit_capture_post_processor"; }

  void SetRuntimeSetting(webrtc::AudioProcessing::RuntimeSetting setting) override {}
};

}  // namespace

std::unique_ptr<webrtc::CustomProcessing> create_capture_post_processor() {
  return std::make_unique<CapturePostProcessor>();
}

}  // namespace livekit_ffi
