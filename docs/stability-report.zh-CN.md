# 稳定性测试报告 —— 数据不会丢、不会缺、算得准

> **对象**：Cella 0.13.0，提交 `2b1e95b`；工作树另含 round 23 之后未提交的本地改动
> （calc / engine / functions / model / xlsx 等 17 个修改文件）。**所有测试都在这台机器的
> 当前实际状态上运行**——这正是日常使用会遇到的状态，不是理想化的干净检出。
> （这批改动在报告完成后已提交为 `9e2299b`…`49e98bd` 的六个 PERF/DOCS 提交；三个 soak
> 发现在干净 `2b1e95b` 上同样复现，结论不受影响。）
> **机器**：Windows x86_64；rustc 1.97.1 (8bab26f4 2026-07-14)；cargo 1.97.1。外部校验
> 依赖：Python 3.11.9 + openpyxl 3.1.5（第三方解析器）、LibreOffice（第三方计算引擎）、
> Excel 16 与 WPS 12（两个独立电子表格应用，经 `cargo xtask oracle` 应用级探针）。
> **日期**：2026-11-20。复现命令见文末[方法与复现](#方法与复现)；原始日志与探针脚本在
> `plan/reports/`（本地，gitignored）。
>
> **2026-11-26 补记**：第一/第四层的"单部件 512 MB"上限已提高为 **2 GiB**（整包 **4 GiB**），
> 而且**用户亲手选的文件按它自己的解压体积开绿灯**（目录声明的体积 + 10%，上限 8 GiB）——
> 起因是它拒绝了用户真实在用的工作簿（112 MB，解压 1.32 GB），而 WPS 能打开；正文相关段落
> 已按新事实改写，第四层表格里的历史计数保留当时结果。
>
> **2026-11-26 第二次补记（显卡驱动崩溃）**：AMD 的 OpenGL 驱动（`atio6axx.dll`）在打开一份
> 112.8 MB 工作簿后让整个进程崩过一次（`0xc0000005`，事件查看器与崩溃转储都在）。这类崩溃是
> "驱动把进程带走"，任何 Rust 代码都拦不住，所以现在按"一定会再发生"来设计：Windows 默认渲染回到
> **DirectX 12**（实测代价：同一份工作簿常驻内存多约 213 MB），每次启动在日志里记下**实际用的显卡与
> 驱动版本**，启动时写一个"我还活着"的标记、正常退出时删掉——下次启动发现标记还在，就自动换另一个
> 后端并在信息栏说明；也可以自己在 文件 ▸ 选项 ▸ 图形 › 渲染器 里指定（自动 / DirectX 12 /
> OpenGL / Vulkan）。详见 [rendering.md](rendering.md)。
>
> **2026-11-27 补记（soak 全绿）**：本报告"结论"表里点到的动态数组缺陷有了下文。11-26 的 **200 × 200**
> 随机命令 soak（40,000 条命令）找到 **9 个不变量失败**，11-27 又用 **300 × 200**（60,000 条命令）复核，
> 多找到 **1 个**——这 10 个全部修掉了，并且每一个都留下了"去掉改动就会失败"的回归测试：
>
> 1. `sheet.moveColumns` 把公式搬到**它的 shape 解析不出来的位置**（shape 是 (文本, 位置) 的缓存），
>    于是该格**没有任何文本**：显示 `#REF!`、存成空公式、重开后在**别的格**读到文本（6 个 seed）；
> 2. 满量重算在"开跑前就拥有区块的数组"长回来时，驱逐了**整张表**上别人的 spill（而且只删区间、留下值），
>    两个数组在八轮里来回翻答案（seed 44）；
> 3. 一张表上的行编辑会把**另一张表**的相对引用平移同一个带宽（`=SUM(J19:N25)` 变成 `=SUM(J17:N23)`，seed 189）；
> 4. `A1#` 这类 **spill 引用不读它的锚点**，所以"`=SUM(A1#)` 与 `A1` 互相成环"这种链答 `#REF!`，
>    而把它标成环的那一遍答 `#CIRC!`（seed 135）；
> 5. 聚合函数**遇到第一个错误就停**，于是"区间里含自己、且更早处还有个 `#N/A`"的公式从没读到过自己
>    那一格，环没登记（seed 233）。
>
> 现在的账是：`cargo xtask ci` **12/12**；200 × 200 与 300 × 200 两个 soak **0 不变量失败**（4 个被预算
> 截断的慢 seed 单独不限预算跑完，各 200 步，同样 0 失败）；十四个失败 capture 全部可回放通过。
> 因此"结论"表里"不变量失败 3 例""另 2 例 spill 状态不一致"这两句**已被修复取代**（原文保留作当时记录）；
>"关键账本暂避会溢出的动态数组公式"这条建议可以收窄为：**已经测到的这几类问题不存在了**，但动态数组与
> spill 仍是全仓测试覆盖最薄的角落，新写法第一次用之前值得拿一份副本试。11-27 还有两条**当天写下又被实测
> 推翻**的结论已经撤回（"区间读不到邻居 spill 的格子"——实测输入本来就是空区间；"种子 44 的五行才对"——
> 那是上面的跨表 bug 喂错了行），见 `docs/gaps.md` §6。
>
> 本报告回答财务类使用者最关心的三个问题：**数据会不会丢、会不会缺、算得准不准**。

## 结论

| 问题 | 结论 | 关键证据 |
| --- | --- | --- |
| **数据不会丢**（崩溃与保存不毁文件） | ✅ 通过 | 保存 = 写临时文件后原子重命名（崩溃不留下半写的文件）；每条命令执行都有 panic 捕获兜底；非测试代码禁止 `unwrap`/`expect`/`panic`，`unsafe` 全仓禁止；本次 2,059 项测试 + 59,120 条随机命令 **0 崩溃**（不变量失败 3 例，全部在动态数组/表格度量场景，见「本轮 soak 新发现」） |
| **数据不会缺**（存 → 开不丢内容） | ✅ 通过 | XLSX 往返保真套件 479 项全绿；随机命令每隔几步做一次存/开全量比对；338 个真实工作簿语料 save→reopen **0 差异**；第三方解析器对两份账本逐格 3,666 项检查 **0 失败**；另 1 例行元数据往返差异已记录在案（非单元格数据，见「本轮 soak 新发现」） |
| **数据保证准确**（计算可复核） | ✅ 通过 | "全量重算 ≡ 增量重算"作为不变量随每轮 soak 检查；微软文档算例 oracle；内建 `cella-cli check` 72 个公式重算全匹配；**Excel 16、WPS、LibreOffice、openpyxl 四个独立实现**对同一份账本交叉一致；另 2 例动态数组 spill 状态不一致已记录在案（见「本轮 soak 新发现」） |

一句话：**在本次实测范围内**——338 个真实世界工作簿、四个独立实现的交叉验证、
59,120 条随机命令——**常规财务数据与公式**（单元格值、SUM/SUMIF/引用链、保存往返）
没有出现任何一处不一致，每个数都能被第三方引擎复核。重压档同时抓到 3 个
动态数组/行元数据场景的缺陷（已确定性复现并留档，见「本轮 soak 新发现」）：它们
不改写上面的结论，但为"放心使用"划出了边界——关键账本暂避会溢出的动态数组公式。
其余已知边界与未覆盖场景见[诚实的边界](#诚实的边界)。

## 第一层：写不坏 —— 崩溃安全设计

（先于测试的结构性保证："数据不会丢"的第一道防线是根本不进入会丢数据的状态。）

- **落盘原子**：`crates/engine/src/io.rs` 的 `write_file` 先写 `{path}.sctmp` 再
  `std::fs::rename`。源码注释原文："Write to a temporary file, then rename: a crash
  never leaves a half-written workbook"。保存中途崩溃/断电，磁盘上的**原文件完好无损**，
  最多丢"最近一次保存之后的改动"；写或改名失败（磁盘满、文件被占用）会清理临时文件，
  不在工作簿旁边留垃圾。
- **panic 兜底**：每条命令的执行都包在 `std::panic::catch_unwind`
  （`crates/engine/src/lib.rs:929`；`:1011` 是第二道）。逃逸的 panic 被捕获、工作簿
  回滚、以 `EngineError::Internal` 返回——而不是进程崩溃带走未保存的工作。这是最后
  一道网，不是许可证：仓库纪律仍然是"非测试代码不 panic"。
- **代码纪律**：`#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, …)]`
  （`engine/src/lib.rs:6`）、`#![forbid(unsafe_code)]`（`:7`，workspace 级
  `unsafe_code = "forbid"`），`cargo xtask ci` 的 clippy `-D warnings` 全仓强制。
- **敌意输入防御**（"input-derived numbers are hostile" 原则的落地）：
  - 批量命令超过 **1,200 万格**直接拒绝（`MAX_BULK_CELLS`，`cmd/mod.rs:439`）；
  - 深递归公式有嵌套上限、工作预算与 896 KiB 栈保护——超限返回错误值而非栈溢出
    （gaps.md C2，2026-11-16 落地；此前深递归会直接中止进程）；
  - 单个 zip 部件解压超 **2 GiB**、整包超 **4 GiB** 拒绝打开——这是防"损坏/恶意文件"的
    上限，不是对真实报告的判断（见第四层的实测案例）：2026-11-26 前它是 512 MB / 1 GB，
    结果把用户 112 MB 的居档工作簿（解压 1.32 GB）挡在门外，而 WPS 能打开。现在**用户亲手
    选的文件**按它自己目录声明的体积开绿灯（+10%，封顶 8 GiB），并有信息条说明为什么慢；
    没经过界面的打开（智能体、脚本、查询源）仍按上限。伪造声明的 10 GB 炸弹照旧在解压前
    被拒，`CELLA_MAX_PART_MB` / `CELLA_MAX_TOTAL_MB` 仍可提额。
- **自动保存**（可选开关）：每次改动后自动写盘（"Save .xlsx workbooks automatically
  after every change"，`ui-egui/src/backstage.rs:987`），配合原子写把未落盘窗口压到
  一次改动的距离；开关偏好本身的持久化有专门测试（`autosave_survives_a_restart`）。

## 第二层：测不倒 —— 随机命令 soak 的不变量体系

`crates/engine/tests/command_sequence_fuzz.rs` 用可复现的随机命令序列轰击样例工作簿
（sales / budget / grades），每一步检查六类不变量（源码头注释全文可查）：

1. 任何命令不 panic；保存、重开、重算同样不 panic；
2. 不可撤销的命令绝不改单元格内容（否则一次撤销就丢数据）；
3. 结构始终合法（活动表在界内、表与表名有效且唯一、合并/表格在表上不重叠、选择在表上）；
4. **全量重算 ≡ 增量重算**——从零重算出的值必须和增量重算留下的值一致（专抓脏依赖图，
   这是"算得准"的引擎级证明）；
5. 每隔几步 + 结束时：XLSX 保存→重开（含打开时的重算）后，工作表、公式与值全部一致
   （这是"不丢不缺"的引擎级证明）；
6. 结束时全部撤销 → 回到初始工作簿；全部重做 → 回到终态（Watch Window 列表这类视图
   状态除外）。

**本次运行**（同一棵工作树）：

| 档位 | 规模 | 结果 |
| --- | --- | --- |
| 门禁档（debug） | 6 seeds × 40 步 | 0.2 s 墙钟，0 不变量失败，0 slow |
| 本次加压（release） | 64 seeds × 120 步 = 7,680 条命令 | 23.1 s 墙钟，0 不变量失败，0 slow，最慢 seed 16.2 s |
| 本次重压（release，KEEP_GOING=1） | 256 seeds × 200 步 = 51,200 条命令 | 318.1 s 墙钟，**3 个不变量失败**（见「本轮 soak 新发现」）、0 slow、最慢 seed 110（53.5 s） |

**历史纵深**（说明这套不变量真的抓得到 bug，不是摆设）：

- 2026-10-27：500 seeds × 200 步（10 万条命令，release，49 分钟）全绿（gaps.md §10）；
- 2026-11-02：100 seeds × 200 步 debug 档 68.3 s 全绿；
- 2026-11-18：soak 抓到 **Remove Duplicates 行压缩后公式 shape 未重定基**——内存里
  `#REF!`、重载后变 0 的真实保真 bug——提交 `30d23d86` 修复并加回归测试
  `a_row_compacted_by_remove_duplicates_keeps_the_references_its_text_names`；发现它的
  8×150 与 16×120 两轮 sweep 复跑通过（STATUS round 23）。

### 本轮 soak 新发现（全部确定性复现）

> **更新（2026-11-20）：三例已全部修复**（提交 `8419bb6`、`3f38117`；每个 bug 先红后绿
> 各一条回归测试；`CELLA_FUZZ_SEEDS=256 CELLA_FUZZ_STEPS=200` release soak 复跑两轮全绿，
> 242.2 s / 244.5 s，0 失败）。下表为发现时的原始记录，保留不删。

256 seeds × 200 步的重压档在种子 51 / 173 / 247 上抓到 **3 个不变量失败**——门禁档
6×40、加压档 64×120 与历史 500×200 都不触发，这正是 soak 预算越高越能挖出问题的又一例
（与 11-18 抓到 Remove Duplicates bug 是同一机制）。三个序列的捕获文件已存
`plan/reports/captures/`，逐个 replay 均 exit=101，**确定性复现**：

| # | 种子 / 样例 | 触发命令 | 不变量 | 现象 | 分类 |
| --- | --- | --- | --- | --- | --- |
| 1 | 51 / sales | `edit.paste`（第 182 步） | recalc | `Sales!E22`（`=SEQUENCE(2,2)`）：内存态 `#SPILL!`，全量重算得 `1.0` | 动态数组状态不一致 |
| 2 | 247 / budget | `cell.set`（第 98 步，写入 `=TRANSPOSE(H23:K30)`） | recalc | `Budget!J5`（`=SORT(F9:H12)`）：内存态 `#SPILL!`，全量重算 `#CIRC!` | 同上 |
| 3 | 173 / grades | `data.measure.add`（第 79 步） | xlsx roundtrip | `Grades (2)`：重载后行元数据不同 | 行元数据保真 |

三个发现的共同点与含义：

- **没有任何一例触及单元格的值或公式本身**：#1、#2 是动态数组（spill）在随机编辑风暴下
  的内存态与重算态不一致（屏幕上可能出现错误值，F9 全量重算或重新打开即恢复）；#3 比较
  的是行的 hidden / outline / 折叠 / 样式 / 自定义行高（fuzz 的 `kept_lines` 比较项）——
  值、公式、工作表名的往返比对全部通过。
- **财务含义**：常规财务公式（SUM / SUMIF / SUMIFS / 引用链——见 3.1 的 72 个公式与全部
  准确性套件）在所有证据中无一处不一致；三个发现都在动态数组溢出与表格度量这类边缘
  场景。**建议**：关键账本暂不使用会溢出的动态数组公式（或在关键步骤后保存重开验证）。
- **最小复现已缩出**（`CELLA_FUZZ_SHRINK=1`）：#1 为 **19 步**（`=SEQUENCE` 溢出历经
  大量行列增删后被一次 `edit.paste` 部分覆盖）；#2 仅 **6 步**（两个溢出公式区域相争 +
  deleteCells/deleteRows/deleteColumns + 再写一个 `=TRANSPOSE`）；#3 为 **8 步**，且最小
  复现**不含** `data.measure.add`——真实路径是"工作表自拷贝 + 筛选 + 表格 + 撤销 +
  filterBy"，疑与 gaps.md §3 已备案的"AutoFilter × 表格筛选共存"案例同族，待三查。
  三份捕获文件在 `plan/reports/captures/`，最小序列输出即回归测试的 `execute` 调用。
  干净检出（`git worktree`，`2b1e95b`，无未提交改动）上三个捕获同样复现——
  **不是未提交改动引入的回归**，是已提交代码中的存量缺陷。已立案 GitHub：
  spill 两例 → [#4](https://github.com/Matt-qwq/cella/issues/4)，行元数据 → [#5](https://github.com/Matt-qwq/cella/issues/5)。
- **复现**（任一份捕获文件；需关掉单 seed 预算，否则被截断的 replay 也判失败）：
  设 `CELLA_FUZZ_SEED_BUDGET_MS=0` 与
  `CELLA_FUZZ_REPLAY=plan/reports/captures/seed-51-sales.txt`
  （`seed-173-grades.txt`、`seed-247-budget.txt` 同理）后运行
  `cargo test -p cella-engine --test command_sequence_fuzz`；加 `CELLA_FUZZ_SHRINK=1`
  可缩到最小序列，输出即回归测试的 `execute` 调用。

## 第三层：算得准 —— 四个独立实现的交叉验证

### 3.1 端到端财务场景（本次新建，可复现）

用 `cella-cli` 从空白工作簿构建一份复式记账账本（探针脚本
`plan/reports/ledger_probe.py`，种子 20261120）：

- **记账凭证**：240 行（12 个月 × 10 笔，每笔一借一贷等额），7 列（凭证号/日期/摘要/
  科目/借方金额/贷方金额/月份），**1,440 个输入值**，两位小数真实金额；
- **科目汇总**：9 个科目的 `SUMIF` 借贷汇总 + 余额 + 合计（跨表引用）；
- **月度损益**：12 个月的 `SUMIFS` 收入/费用/结余 + 合计；
- 顶层控制公式：`=SUM(借方)`、`=SUM(贷方)`、
  `=IF(E243=F243,"平衡","不平衡")`（借贷平衡检查——双录等额，必须精确相等）；
- 共 **72 个公式**，中文表名、中文文本。

四方验证结果：

| 验证方 | 角色 | 检查数 | 结果 |
| --- | --- | ---: | --- |
| `cella-cli check` | Cella 自检：打开→全量重算 vs 缓存 + 存/开往返全类目 | 72 公式 + 12 类目 | 72/72 匹配，`staleCache: false`，往返 0 差异 |
| **openpyxl**（第三方解析器） | 对 Cella 直存文件逐格核对（Python 独立重算期望值） | 1,833 | **0 失败**：311 个数值**逐位精确**、文本/科目/月份精确 |
| **openpyxl**（重载链） | Cella 存→开→再存的第二份文件，同样核对 | 1,833 | **0 失败**，与直存文件完全一致 |
| **LibreOffice**（第三方引擎） | 完全独立重算同一文件 | 1,833 | **0 失败**：287 逐位精确 + 24 在 1e-9 相对容差内 |

所有 240 个日期格被 Cella 按 Excel 习惯从 ISO 文本转成真正的日期类型
（openpyxl 读回 `datetime`，语义逐格一致）——探针首跑把这 240 格记作"失败"，查明是
上述 Excel 兼容行为后修正判据；这本身就是一次兼容性佐证。两份 Cella 文件（直存与
重载链）的逐格校验**每项都一致**，说明存→开→存不产生任何漂移。

### 3.2 标准样例的应用级往返（`cargo xtask oracle`，本次运行）

5 个样例工作簿（sales / budget / grades / features / pivot），每个过三道探针：

| 探针 | sales | budget | grades | features | pivot |
| --- | --- | --- | --- | --- | --- |
| openpyxl 读回（外部解析器） | ok | ok | ok | ok | ok |
| **Excel 16 应用往返** | ok | ok | ok | ok | ok |
| **WPS 12 应用往返** | ok | ok | ok | ok | ok |

"应用往返"= 另一个电子表格应用打开 Cella 写的文件 → 刷新全部透视表 → 重算 → 存出
副本 → Cella 读回副本并逐值比对（含格式、合并、表格、CF/DV/图表数量、名称）。**5 个
样例 × 2 个应用全部存活**——包括带分组数值字段、值筛选器和切片器的透视表。

### 3.3 计算准确性的静态防线

- **微软文档 oracle**（`crates/calc/tests/formula_oracle.rs`，9 项测试全过）：期望值
  逐条转录自 learn.microsoft.com 的函数参考；容差取"文档打印值最后一位的一半"，绝不为
  迁就实际输出而放宽。源码头注释诚实声明它弱于 Excel 实机——实机那半边由 3.2 的
  Excel 16 应用探针补齐。
- **15 位有效数字相等**（#57 已闭环）：`=0.1+0.2=0.3` 为 TRUE——比较按 Excel 的
  15 位语义，MATCH / COUNTIF 与之一致。
- **"按显示精度取整"真正生效**（C1）：`precision_as_displayed`（5 项）、
  `round_as_displayed`、`render_equality` 等套件全绿——财务报表的显示位数与重算
  结果一致。

## 第四层：真实世界的工作簿语料

对本机真实工作文件夹（`D:/baidutongbu/BaiduSyncdisk/Matt/数据`，9.6 GB 同步盘）
重跑 `cargo xtask corpus`：打开每个可读 xlsx，按比例存→重开→digest 比对→openpyxl
外读（**从不写入语料目录**）：

| 目录 | 文件 | 打开 | 失败 | 存→重开 | digest 差异 |
| --- | ---: | ---: | ---: | ---: | ---: |
| 佛山（`--all` 全存） | 20 | 20 | 0 | 20 | **0** |
| 黄埔 | 313 | 312 | 1（见注） | 63 | **0** |
| 增城（`--all`） | 2 | 2 | 0 | 2 | **0** |
| 从化 / 越秀（`--all`） | 3 | 3 | 0 | 3 | **0** |
| **本次合计** | 338 | 337 | 1 | 88 | **0** |

> 注：黄埔唯一的"失败"是 Cella 当时**拒绝**打开一个 72.8 MB 的工作簿——其 `sheet2.xml`
> 解压后 608 MB、整包 622 MB，超过当时单部件 512 MB / 整包 1 GB 的上限。这个判断在 2026-11-26
> 被推翻：同类的 112 MB 居档工作簿（解压 1.32 GB、单部件 619 MB）在用户机器上被拒，而 WPS
> 能打开——被挡住的是真实在用的档案，不是缺陷文件。现在这个黄埔工作簿与那份居档工作簿都能
> 打开（CLI 0.99 s / 2.03 s；桌面版 3.2 s，显示 7.58 M 格的工作表）。更大的也一样：219.4 MB
> 的荔湾华林就诊记录（62.7 M 格，解压 1,895 MB）桌面版打开，首次 17.3 s（要写分页文件）、之后
> 6.5–7.0 s，常驻只要 258–311 MB——内存跟着分页文件走，不跟着工作簿走。
> 防御本身仍在：伪造声明的炸弹在解压前被拒，超过 8 GiB 声明的文件打开时也是拒绝而不是尝试。

**历史全量**（2026-11-03，同目录 3,199 个工作簿，提交 `09397e79`，详录
`plan/corpus-run-2026-11-03.md`）：3,084 个可读工作簿全部打开，**635 次 save→reopen
0 digest 差异**（digest 覆盖值、公式文本、样式、合并、批注、链接、数据验证/条件格式
范围、表格、名称、图表、形状与图片）；Excel 16 对存出的副本逐公式/常量比对
**16/16 完全一致**；37 个可读 `.xls` 转换后 36 个内容完全一致、1 个仅格式列不同
（与 Excel 自己的 `.xls`→`.xlsx` 转换行为一致）。115 个失败文件全部是输入侧问题
（4 个截断的 zip——openpyxl 同样拒绝；111 个带密码）。

## 第五层：全量门禁（release 前必跑）

`cargo xtask ci` 九步：`fmt --all -- --check`（只读检查）、
`clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`（含 UI 的
903 项）、connectors 特性测试、assets、layers、wasm32、parity/keytips/i18n 校验。

**本次结果（2026-11-20）：9/9 全部通过。** `fmt --all -- --check` 零改动、
`clippy --workspace --all-targets -- -D warnings` 零警告、`cargo test --workspace`
221 个测试套件 **3,666 项通过 / 0 失败**（含 UI 的 903 项与 connectors 特性测试）、
assets / layers / wasm32 / parity / keytips / i18n（509 条标签全带中文）全部 ok。

## 测试总账（本次实测）

| 套件 | 命令 | 结果 |
| --- | --- | --- |
| XLSX 读写/往返保真 | `cargo test -p cella-xlsx` | 479 通过 / 0 失败 / 2 个计时探针跳过 |
| 引擎命令全套（55 个测试二进制） | `cargo test -p cella-engine` | 851 通过 / 0 失败 |
| 计算准确性（21 个测试二进制） | `cargo test -p cella-calc -p cella-functions -p cella-formula -p cella-numfmt -p cella-model -p cella-core` | 705 通过 / 0 失败 / 2 个计时探针跳过 |
| fuzz 回归集（历史 bug 的回归测试） | `cargo test -p cella-engine --test fuzz_regressions_…` | 21 通过 / 0 失败 |
| 命令序列 fuzz | 见第二层 | 0 不变量失败 |

合计 **2,059 项测试全绿**（另有 4 个明确标注"计时、非检查"的探针跳过）。准确性套件里
与财务最相关的：`formula_oracle` 9 项（微软文档算例）、`budgets` 8 项（预算场景）、
`precision_as_displayed` 5 项、`round_as_displayed`（按显示精度取整）、
`adjust_oracle` 3 项（结构改动后的引用重定基）、`printer_roundtrip`（公式文本往返）。

## 诚实的边界

全绿 ≠ 无限承诺。以下是**当前已知、已记录在案**的边界，财务使用请对照：

1. **本轮 soak 新发现的 3 个缺陷**（2026-11-20，见「本轮 soak 新发现」）：两例动态数组
   spill 的内存态与全量重算不一致、一例行元数据往返差异（最小复现不含 measure.add，
   疑与 gaps §3 已备案案例同族）。值与公式本身无任何不一致；三个复现在干净检出
   `2b1e95b` 上同样失败（存量缺陷，非未提交改动引入），已立案 [#4](https://github.com/Matt-qwq/cella/issues/4)、[#5](https://github.com/Matt-qwq/cella/issues/5)；
   修复前，关键账本避免会溢出的动态数组公式。
2. **同一张表上 AutoFilter 与表格筛选共存**（gaps.md §3，已知未修）：列删除后再加第二
   个筛选，保存/重开会出现隐藏行差异。六步可复现（seed 97；150×120 的 soak 才会碰到，
   门禁档 6×40 不触发）。**规避**：一张表用一种筛选，或加第二种筛选前先清掉第一种。
3. **错误值缓存**（gaps.md §10 遗留）：文件格式无法缓存 `#SPILL!`/`#CALC!`（Cella 写
   `#VALUE!`，按"Excel 拒写这些码"的记载，**尚待有授权的 Excel 实机验证**），
   `#CIRC!` 缓存为 0。公式与溢出值重算后一致，但"字面量粘贴的错误码"重开会变样。
   常规财务报表不涉及这些动态数组错误码。
4. **数值模型是 IEEE-754 f64**——与 Excel 完全相同的模型，不是任意精度小数。需要"分"
   级绝对精确的台账：用整数（分）入账或配 `ROUND`；Excel 用户对浮点的全部注意事项在
   Cella 同样适用。
5. **未覆盖**：多用户并发编辑/共享工作簿、网络盘同步冲突（语料是单用户本地盘）、
   保存期间强行断电的物理实验（原子重命名在文件系统语义上保证旧文件完好，未做破坏性
   实测）。
6. **上游字段机的崩溃报告**（gaps.md §10 的 #120 等）来自上游仓库的机器，不属于本仓库，
   未在本机复现。
7. **语料是本地盘证据**：固化（sha256 钉住、可分享）的 pinned corpus 仍是待办
   （gaps.md §1）；Excel 侧 owner 人工检查清单 #7/#8/#10 未尽
   （[excel-checks.md](excel-checks.md)）。
8. **本报告的树不是干净检出**：round-23 后 5 个提交 + 17 个文件的本地改动都参与了本次
   测试。按 STATUS 的要求，release 前要重跑完整门禁——本报告第五层就是那次重跑。

## 给财务场景的使用建议

1. **开启自动保存**：每次改动后原子落盘，未保存窗口最小化；
2. **关键账本保留周期副本**（Save As 或直接复制文件）：原子写保证不被写坏，但 Cella
   不做版本历史——版本冗余靠用户自己的备份习惯；
3. **账本内置自校验**：像 3.1 的场景一样放一行
   `=IF(借方合计=贷方合计,"平衡","不平衡")`，平衡性断裂时第一眼可见；
4. **随手体检**：`cella-cli check 账本.xlsx --json` 一条命令完成"打开→重算→往返"
   自检（本报告 72/72 全过用的就是它）；
5. **大操作先试副本**：批量命令有 1,200 万格硬上限兜底；百万行级工作簿的性能预算另见
   [performance-report.zh-CN.md](performance-report.zh-CN.md)。

## 方法与复现

全部命令在仓库根目录运行；逐命令日志在 `plan/reports/*.log`：

| 目的 | 命令 |
| --- | --- |
| XLSX 往返套件 | `cargo test -p cella-xlsx` |
| 引擎全套 | `cargo test -p cella-engine` |
| 准确性套件 | `cargo test -p cella-calc -p cella-functions -p cella-formula -p cella-numfmt -p cella-model -p cella-core` |
| fuzz 回归 | `cargo test -p cella-engine --test fuzz_regressions_empty --test fuzz_regressions_recalc --test fuzz_regressions_roundtrip --test fuzz_regressions_structure --test fuzz_regressions_tables` |
| 门禁档 fuzz | `cargo test -p cella-engine --test command_sequence_fuzz -- --nocapture` |
| 加压 soak | 环境变量 `CELLA_FUZZ_SEEDS`/`CELLA_FUZZ_STEPS`/`CELLA_FUZZ_SEED_BUDGET_MS` 后 `cargo test --release -p cella-engine --test command_sequence_fuzz -- --nocapture` |
| 财务场景 | `python plan/reports/ledger_probe.py gen` → `cella-cli run --script plan/reports/ledger-cmds.jsonl --out ledger-e2e.xlsx` → `cella-cli check ledger-e2e.xlsx --json` → `cella-cli convert ledger-e2e.xlsx ledger-reloaded.xlsx` → `python plan/reports/ledger_probe.py verify <file>` |
| 第三方引擎重算 | LibreOffice 重算（office_recalculate）后用同一探针校验 |
| 应用级 oracle | `cargo xtask oracle` |
| 真实语料 | `cargo xtask corpus <dir> --recursive [--all]` |
| 全量门禁 | `cargo xtask ci` |
| 失败复现 | 环境变量 `CELLA_FUZZ_SEED_BUDGET_MS=0` 与 `CELLA_FUZZ_REPLAY=plan/reports/captures/<捕获文件>` 后 `cargo test -p cella-engine --test command_sequence_fuzz`（加 `CELLA_FUZZ_SHRINK=1` 缩最小序列） |

> 方法论备注：多命令链的输出要逐条重定向——本次 从化/越秀 首跑的逐行汇总因链尾
> 重定向丢失，已按单目录单日志补跑（1 + 2 个文件，0 差异）。
> `plan/reports/` 在 gitignore 内（本地证据；三个失败捕获文件另存
> `plan/reports/captures/`）；报告结论以命令为准，均可复现。
