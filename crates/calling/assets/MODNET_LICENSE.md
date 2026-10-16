# modnet_webcam_portrait_matting.onnx

| Item | Value |
|---|---|
| Model | MODNet webcam portrait matting (Ke et al., "MODNet: Real-Time Trimap-Free Portrait Matting via Objective Decomposition", AAAI 2022) |
| Author | Zhanghan Ke and contributors, https://github.com/ZHKKKe/MODNet |
| License | Apache License 2.0, https://www.apache.org/licenses/LICENSE-2.0 (MODNet README: "The code, models, and demos in this repository ... are released under the Apache License 2.0") |
| Weights | `pretrained/modnet_webcam_portrait_matting.ckpt`, mirrored at https://huggingface.co/DavG25/modnet-pretrained-models (Apache-2.0) |
| Input | `input` float32 [1, 3, height, width], RGB, (pixel - 127.5) / 127.5, height and width multiples of 32 |
| Output | `matte` float32 [1, 1, height, width], alpha 0..1, 1 = person |
| Changes | exported to ONNX opset 17 with `MODNet.forward(image, inference=True)[2]` and dynamic height and width; weights stored as float16 with a Cast to float32 in front of each (ONNX Runtime folds the casts at load) |
