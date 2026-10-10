# selfie_segmentation_landscape.onnx

| Item | Value |
|---|---|
| Model | MediaPipe Selfie Segmentation, landscape (256x144) |
| Author | Google LLC (MediaPipe) |
| License | Apache License 2.0, https://www.apache.org/licenses/LICENSE-2.0 |
| ONNX conversion | https://huggingface.co/onnx-community/mediapipe_selfie_segmentation_landscape (`onnx/model.onnx`, Apache-2.0, repo revision 2497d5bec26c626c7b3c4edc6e1fefc21b64f6c3) |
| Input | `pixel_values` float32 [1, 3, 144, 256], RGB, 0..1 |
| Output | `alphas` float32 [1, 1, 144, 256], person probability |
| Changes | none; the file is used as downloaded |
