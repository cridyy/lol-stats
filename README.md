# 国服战绩分析

一个专注于国服英雄联盟客户端的战绩统计工具。项目使用 Tauri 2 + Rust + Vue 3 开发，目标是把功能收束在两件事上：

- 查询当前账号或指定玩家的历史战绩、英雄统计和单英雄详情。
- 实时对局中读取双方玩家的近期战绩，帮助快速判断阵容和玩家近期状态。

> 当前仅面向国服客户端开发和测试。

## 功能概览

- 自动发现本机正在运行的 `LeagueClientUx.exe`，读取 LCU / Riot Client 连接参数。
- 当前角色页：
  - 显示当前登录账号的近期战绩。
  - 支持滚动到底部按 20 局一组继续加载。
  - 显示单双排、灵活组排及历史最高段位。
  - 数据统计支持 50 / 100 / 200 / 300 / 400 / 500 场样本。
- 查战绩页：
  - 支持 Riot ID / PUUID 查询。
  - 支持国服区服下拉选择。
  - 查询历史本地保存，可删除。
  - 查询结果复用当前角色页的展示逻辑。
- 数据统计：
  - 单英雄场次、胜率、K/D/A。
  - 伤害占比、伤害转化率、承伤占比、治疗占比。
  - 英雄排行可点击进入该英雄对应模式的具体战绩。
- 实时战绩：
  - 自动刷新当前选人或游戏中对局。
  - 我方始终排在上方，敌方排在下方。
  - 按当前对局模式筛选历史战绩：排位、匹配、大乱斗、海克斯大乱斗分别使用对应口径。
  - 每名玩家先请求近 20 场，不够再继续请求，最多扫描近 100 场并凑 20 条有效战绩。
  - 展示团队平均胜率、平均伤害、平均 KDA、平均伤转和平均承伤。
- 详细战绩：
  - 点击具体对局可打开总览标签页。
  - 总览展示双方 10 人的英雄、技能、装备、符文/强化、K/D/A 和关键数据。
  - 可继续点击对局内玩家，在“详细战绩”页循环打开该玩家战绩标签。
- 分享截图：
  - 对局总览、单英雄统计、单英雄具体战绩支持生成图片并复制到剪切板。
  - 默认开启手机版截图布局。
- 本地缓存：
  - 使用 SQLite 保存统计缓存和对局索引。
  - 按 `sgpServerId + puuid + depth` 隔离缓存，避免跨区服污染。

## 技术栈

- Tauri 2
- Rust
- Vue 3
- TypeScript
- Vite
- SQLite / rusqlite
- lucide-vue-next

## 目录结构

```text
lol-stats
├─ src
│  ├─ api.ts                    # Tauri 命令调用封装
│  ├─ assetLoader.ts            # LCU 资源加载和前端缓存
│  ├─ imageShare.ts             # 截图复制工具
│  ├─ notifications.ts          # 全局通知
│  ├─ types.ts                  # 前端类型
│  ├─ utils.ts                  # 前端格式化工具
│  ├─ App.vue                   # 主界面和页面编排
│  └─ components
│     ├─ GameRecordList.vue     # 通用战绩横条
│     ├─ LiveGamePanel.vue      # 实时战绩面板
│     ├─ MatchOverviewPanel.vue # 单局对局总览
│     ├─ PlayerRecordTab.vue    # 详细战绩中的玩家标签页
│     ├─ RecordView.vue         # 当前角色 / 查询结果通用视图
│     ├─ StatsPanel.vue         # 数据统计和单英雄统计
│     └─ ToastStack.vue         # 顶部通知
└─ src-tauri
   └─ src
      ├─ commands.rs            # 暴露给前端的 Tauri 命令
      ├─ lib.rs                 # Tauri 入口
      └─ services
         ├─ client_discovery.rs # LeagueClientUx 发现与命令行解析
         ├─ error.rs            # 后端统一错误
         ├─ lcu.rs              # LCU / Riot Client HTTP 封装
         ├─ models.rs           # 响应模型和前端输出模型
         ├─ sgp.rs              # 国服 SGP 深战绩请求
         └─ stats.rs            # 战绩读取、缓存和聚合逻辑
```

## 开发运行

先确认国服英雄联盟客户端已经登录。

以下命令以 `lol-stats` 项目目录为工作目录；如果从仓库根目录开始，先执行：

```pwsh
cd lol-stats
```

安装依赖：

```pwsh
npm install
```

启动开发模式：

```pwsh
npm run tauri dev
```

只检查前端：

```pwsh
npm run build
```

只检查 Rust 后端：

```pwsh
cd src-tauri
cargo check
```

## 打包

项目默认使用 NSIS 生成 Windows 安装包：

```pwsh
npm run tauri build
```

打包产物默认位于：

```text
lol-stats/src-tauri/target/release/bundle/nsis/lol-stats_0.1.0_x64-setup.exe
```

同时会生成 release 主程序：

```text
lol-stats/src-tauri/target/release/lol-stats.exe
```

## 数据来源与连接方式

当前版本主要使用本机客户端暴露的接口：

- LCU HTTP：`https://127.0.0.1:{app-port}`
- Riot Client HTTP：`https://127.0.0.1:{riotclient-app-port}`
- 国服 SGP 深战绩：`https://{rso-platform-id}-sgp.lol.qq.com:21019`
- 认证方式：`Basic base64("riot:{token}")`

LCU / Riot Client 连接参数来自 `LeagueClientUx.exe` 的命令行参数：

- `--app-port`
- `--remoting-auth-token`
- `--region`
- `--rso_platform_id`
- `--riotclient-app-port`
- `--riotclient-auth-token`

如果常规进程接口读取不到命令行，后端会读取 Windows PEB 中的 CommandLine。

国服跨区服查询使用 `sgpServerId` 做内部标识，例如：

- `TENCENT_HN1`：艾欧尼亚
- `TENCENT_HN10`：黑色玫瑰
- `TENCENT_NJ100`：联盟一区
- `TENCENT_GZ100`：联盟二区
- `TENCENT_CQ100`：联盟三区
- `TENCENT_TJ100`：联盟四区
- `TENCENT_TJ101`：联盟五区
- `TENCENT_BGP2`：峡谷之巅

## 统计口径

- 会过滤人机、教程等 PVE 队列。
- 会过滤游戏时长小于 8 分钟的对局。
- 伤害转化率 = 团队伤害占比 / 团队经济占比，界面显示为小数倍率。
- 承伤占比 = 个人自我缓和伤害 / 团队自我缓和伤害。
- 治疗占比 = 个人治疗量 / 团队治疗量。
- 实时战绩会根据当前对局模式筛选历史样本，避免用大乱斗数据评价排位，或用排位数据评价海克斯大乱斗。

## 本地持久化

SQLite 默认路径：

```text
%LOCALAPPDATA%\lol-stats\lol-stats.sqlite3
```

如果取不到 `%LOCALAPPDATA%`，会回退到当前工作目录：

```text
.lol-stats/lol-stats.sqlite3
```

当前主要缓存内容：

- `stats_cache`：聚合后的玩家统计结果。
- `match_cache`：对局 summary 原始 JSON。
- `player_match_index`：玩家和对局的索引关系，用于增量读取。

缓存和索引按 `sgp_server_id + puuid` 区分，查询其他区服玩家时不会和当前登录区服混在一起。

## 致谢

这个项目在国服客户端连接、LCU/RC 参数解析、SGP 请求和实时战绩设计上参考了以下开源项目。没有这些项目的探索，本项目会少走得慢很多。

- [LeagueAkari](https://github.com/LeagueAkari/LeagueAkari)：成熟的英雄联盟客户端工具项目，提供了大量 LCU / Riot Client / 国服环境处理思路。
- [rank-analysis](https://github.com/wnzzer/rank-analysis)：国服战绩和段位分析相关项目，帮助确认了部分接口、区服和数据处理方案。

感谢两位项目作者和社区贡献者。

## 免责声明

本项目不是 Riot Games、腾讯游戏或英雄联盟官方项目，也未与官方存在任何隶属关系。

项目只读取本机客户端开放的 LCU / Riot Client 接口和国服战绩接口，不修改游戏数据。请自行承担使用第三方工具的风险，并避免高频请求对服务造成压力。
