/*
 * Copyright 2025 LiveKit, Inc.
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

#pragma once

#if defined(WEBRTC_MAC) && !defined(WEBRTC_IOS)

#include <memory>

#include "modules/desktop_capture/desktop_capture_options.h"
#include "modules/desktop_capture/desktop_capturer.h"

namespace livekit_ffi {

enum class SckCaptureStatus {
  kActive,
  kPickerCancelled,
  kStartFailed,
  kStoppedByUser,
  kWindowGone,
  kInterrupted,
};

// ScreenCaptureKit single-window DesktopCapturer backed by SCContentSharingPicker.
// WebRTC's DesktopCapturer::CreateWindowCapturer on macOS only creates legacy
// WindowCapturerMac (CGWindowList without a system picker).
class SckWindowCapturer : public webrtc::DesktopCapturer {
 public:
  virtual SckCaptureStatus last_status() const = 0;
};

bool SckWindowCapturerAvailable();

std::unique_ptr<SckWindowCapturer> CreateSckWindowCapturer(
    const webrtc::DesktopCaptureOptions& options);

}  // namespace livekit_ffi

#endif  // defined(WEBRTC_MAC) && !defined(WEBRTC_IOS)
