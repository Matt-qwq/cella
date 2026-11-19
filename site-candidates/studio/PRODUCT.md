# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

Cella 官网面向已经会用电子表格的人。不按表格入门课程编写，也不要求访客已经用过 Cella。

- **程序员**：用 CLI 读取、转换或批量修改工作簿，把重复操作接进脚本；需要时通过 MCP 或控制协议连接现有工具。
- **数据分析人员**：打开已有工作簿或导入数据，核对公式，用表格、筛选、数据透视表和图表汇总结果。
- **电商从业者**：处理订单、销售或库存导出表，按商品、渠道或时段汇总和比较。这里描述的是表格工作，不承诺电商平台直连。

## Product Purpose

Cella 是用 Rust 开发的独立开源电子表格。它让用户既能在桌面上处理收到的工作簿，也能把重复的表格任务交给程序。官网成功的标准是：访客能判断 Cella 是否适合手头的文件和工作流，能照着说明开始使用，也能找到项目源代码和文档。

## Positioning

核心差异不是另做一套自动化接口，而是让桌面操作与 CLI、MCP、控制通道使用同一套命令模型。项目优先做深真实文件兼容、数据处理和自动化；Excel 是兼容性参照，不是品牌或完整功能承诺。见[产品定位](../../docs/roadmap.md#product-position-2026-11-17)。

## Operating Context

- 常见输入来自他人发来的现有工作簿或业务导出文件。用户通常需要先确认兼容性，在副本上验证，再决定是否纳入日常流程。
- 桌面入口和无界面 CLI 是两种使用方式；[CLI](../../docs/cli.md)、[MCP](../../docs/mcp.md)和[控制协议](../../docs/control-protocol.md)各有自己的实际使用说明。
- 应用面向 Windows、macOS、Linux 和 BSD，并有 WebAssembly 构建。应用平台支持、某个版本的二进制下载，以及官网静态演示是三件不同的事。
- 官网默认使用简体中文，并提供完整的站内英文版本。

## Capabilities and Constraints

- 读写 `.xlsx` 和 CSV，支持公式、动态数组、表格、数据透视表和图表。不同格式及特性的覆盖范围以[文件格式说明](../../docs/file-format-parity.md)为准。
- 项目当前为 alpha。不要声称完整兼容 Excel，也不要把某次性能测量推广到所有机器或工作簿。
- 仅把 [GitHub Releases](https://github.com/Matt-qwq/cella/releases) 实际列出的产物写成可下载。官网的截图和交互说明不是在线编辑器，不承诺浏览器试玩。

## Brand Commitments

产品名为 Cella，使用项目自己的图标和真实应用素材。Cella 是 GridCraft 的独立修改版，保留[上游致谢](../../NOTICE)，不使用上游或第三方品牌资产。代码许可为 MIT OR Apache-2.0。

## Evidence on Hand

- 原创应用截图：[销售样例](../../docs/images/hero-sales.png)、[Insert 功能区](../../docs/images/dark-insert.png)、[数据透视表](../../docs/images/pivot.png)；来源和许可见[资产归属](../../ATTRIBUTION.md)。
- 可复现的启动命令见 [README 快速开始](../../README.md#quick-start)，性能主张以[测试场景和方法](../../docs/performance.md)为依据。
- [GitHub 仓库](https://github.com/Matt-qwq/cella)是源代码、文档、发布记录及问题反馈的官方入口。

## Product Principles

1. 假定访客懂表格，讲清 Cella 的做法和限制，不从单元格基础教起。
2. 用一个具体任务解释用法，让人能在自己的文件上复现。
3. 以真实截图、可运行命令和有出处的兼容性说明作证，不把原型演示当成产品功能。
