#pragma once

#include <memory>

#include "api/audio/audio_processing.h"

namespace livekit_ffi {

std::unique_ptr<webrtc::CustomProcessing> create_capture_post_processor();

}  // namespace livekit_ffi
