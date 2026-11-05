# 第三方组件与素材

LOL Stats 自行编写的源码按根目录的 [MIT License](LICENSE) 发布。以下组件与素材保留各自许可证或权利声明。

## ncnn

- 上游项目：[Tencent/ncnn](https://github.com/Tencent/ncnn)。
- 使用范围：`src-tauri/ncnn/lib/ncnn.lib` 与 `src-tauri/src/services/hex_winrate/ncnn.rs` 调用的 OCR 推理能力。
- 许可证：[BSD 3-Clause](src-tauri/ncnn/LICENSE.txt)。
- 本仓库保留已有的 Windows 原生静态库，供当前工程构建。其历史编译参数和精确上游版本没有随原始文件记录，不能据此声称可重现该二进制。

## PaddleOCR / PP-OCRv5

- 上游项目：[PaddlePaddle/PaddleOCR](https://github.com/PaddlePaddle/PaddleOCR)。
- 使用范围：`src-tauri/resources/ocr/` 中的 PP-OCRv5 移动端文字识别模型、ncnn 转换参数及字符表。
- 许可证：[Apache License 2.0](src-tauri/resources/ocr/LICENSE.PaddleOCR.txt)。
- 本仓库保留并分发适用于当前推理代码的转换文件；精确的转换命令与上游权重版本没有随原始文件记录。重新转换模型时应同步记录来源、转换参数并验证输出类别数与字符表一致。

## 海克斯设计素材

- 素材来源：[augmentcrafter.org](https://augmentcrafter.org/) 及其开源工程。
- 随素材保留的许可文件：[LICENSE.augmentcrafter](src/assets/hextech/LICENSE.augmentcrafter)。

英雄联盟名称、商标、英雄形象、装备、段位及海克斯图标的相关权利归 Riot Games、腾讯游戏或相应权利方所有。项目源码的 MIT 许可证不代表对第三方游戏素材重新授权。

## ARAMKit

胜率与海克斯统计来自 [ARAMKit](https://aramkit.com/zh-CN)。网站页面与统计数据保留原网站或相应权利方的权利；本仓库的源码许可证不对这些内容重新授权。

## JavaScript 与 Rust 依赖

其他依赖及版本分别记录在 `package-lock.json` 和 `src-tauri/Cargo.lock` 中，各依赖保留其自身许可证。
