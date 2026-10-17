# 数据来源与第三方组件

元器件原始数据来自“嘉立创EDA/EasyEDA官方库”：
- https://lceda.cn/
- https://easyeda.com/

联网获得的数据及 tests/fixtures 中的回归样例保留上游 HEAD.license 版权字段；导出的 Doc 中记录来源链接及原始版权信息。源数据及上游服务遵循各自条款。

本项目的源数据解析器和 Altium ASCII 写入器独立实现，当前版本不依赖 npnp，不包含 npnp 源码、模板或二进制。

公开格式参考：
- 嘉立创EDA文件格式：https://prodocs.lceda.cn/cn/format/index/
- KiCad Altium 格式技术说明：https://dev-docs.kicad.org/en/import-formats/altium/index.html
- Altium ASCII 原理图支持说明：https://www.altium.com/documentation/knowledge-base/altium-designer/text-string-objects-cannot-be-selected-in-schematic-editor
- Altium 原理图字段格式说明：https://github.com/vadmium/python-altium/blob/master/format.md
- ASCII 文档字段交叉核对样例：https://github.com/github-linguist/linguist/tree/main/samples/Altium%20Designer
- PCB ASCII 字段交叉核对样例：https://github.com/LispEngineer/stm-midi-poc1

这些资料用于了解格式字段，未把其他项目的转换实现或整份文件模板复制到本项目。
通用 Rust 依赖及版本见 Cargo.toml / Cargo.lock，遵循各自原始许可。立创和 Altium 商标归各权利人所有，本工具非官方软件。
