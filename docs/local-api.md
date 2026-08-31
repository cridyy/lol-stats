# 本机数据接口

软件启动并完成页面初始化后，会在本机监听：

```text
http://127.0.0.1:17390
```

接口只绑定回环地址，不提供网页跨域响应头；带有 `Origin` 的战绩请求会被拒绝。它面向本机脚本、自动化工具和本地数据看板，不是远程服务。

## 健康检查

```text
GET /health
```

示例：

```powershell
Invoke-RestMethod 'http://127.0.0.1:17390/health'
```

响应：

```json
{
  "ok": true,
  "apiVersion": 1,
  "address": "127.0.0.1:17390",
  "ready": true
}
```

`ready: true` 表示前端评分桥接已经初始化，可以请求玩家数据。

## 玩家模式战绩

```text
GET /v1/player-history?gameName={游戏ID}&tagLine={编号}&server={区服}&mode={模式}&count={场数}
```

示例：

```powershell
$uri = 'http://127.0.0.1:17390/v1/player-history'
$query = '?gameName=舒适的荒野&tagLine=40087&server=TENCENT_HN1&mode=hex-aram&count=100'
Invoke-RestMethod ($uri + $query)
```

参数：

| 参数 | 说明 |
| --- | --- |
| `gameName` | 游戏 ID；不能包含 `#` |
| `tagLine` | 编号；不能包含 `#` |
| `server` | 具体国服区服，例如 `TENCENT_HN1`；不接受 `ALL` |
| `mode` | `hex-aram`、`aram`、`ranked`、`normal`、`arena` 或 `all` |
| `count` | 目标场数，`1-1000` |

模式也接受常用别名：`2400` / `海克斯大乱斗`、`450` / `大乱斗`、`排位`、`匹配`、`斗魂竞技场`、`全部`。

接口会继续扫描最近历史，直到凑齐对应模式的目标场数、没有更多可用历史，或达到软件的 1000 场扫描上限。`coverage.returnedGames` 是最终用于分析的有效对局数，不等于底层原始历史条数；短局、PVE 和无效数据会被过滤。

## 顶层响应

成功响应：

```json
{
  "ok": true,
  "data": {
    "apiVersion": 1,
    "generatedAt": "2026-08-31T00:00:00.000Z",
    "request": {},
    "coverage": {},
    "player": {},
    "globalBaseline": {},
    "summary": {},
    "profile": {},
    "championProfiles": [],
    "games": []
  }
}
```

### `coverage`

```json
{
  "returnedGames": 100,
  "scannedGames": 137,
  "exhausted": false,
  "complete": true
}
```

`complete: false` 表示可用历史不足请求目标，不代表接口出错。

### `globalBaseline`

这是本次响应使用的全服快照摘要：

- `datasetVersion`、`schemaVersion`、来源和生成时间。
- `expectedChampionCount`、`championCount`、`unavailableChampionCount`、`partial`。
- 根节点 `globalAverages`：根据英雄均值和“较平均”字段反算后取中位数的全服人均估计。
- `globalAverageEstimation`：反算公式、候选数量和排除数量。
- `champions`：每英雄基准以及 ARAMKit 的 `relativeToGlobalAveragePercent`。

当前快照保存每英雄 21 项数据，但正式百分制全服分只在海克斯大乱斗辅助分支使用六项功能数据。详情见[全服基准](global-baselines.md)。

### `summary` 和 `profile`

`summary` 是总览快捷字段：场次、胜负、胜率、KDA、正式综合分、评价、主定位和画像标签。

`profile` 是完整玩家画像，包括中位数、高光率、战犯率、波动、定位分布、输出/前排/辅助能力线和最多 10 个画像标签。画像使用软件当前正式评分和时间加权规则，详情见[玩家画像](profile-system.md)。

### `championProfiles`

数组按英雄分组，包含场次、胜负、胜率、KDA、单英雄画像和常用平均指标。单英雄评分和标签规则与界面统计页一致。

## `games[]` 单局结构

每局只返回软件加工后的视图，不返回 SGP 原始 `Game` 对象、原始参与者数组或客户端鉴权信息。

主要分组：

```text
match
  gameId / playedAt / durationSeconds / queueId / mode / win
  participants / comparison
champion
performance
  combat / damage / gold / mitigation / healing / shielding / control
score
  value / evaluation / tag / title / titles / titleDetails / factTags
  role / parts / metrics / scoringMetrics / matchContext
  globalBenchmark / championBaseline / details
```

### `performance`

`performance` 保留界面和脚本常用的原始数值与占比：

- `combat`：击杀、死亡、助攻、KDA、人头占比、参团率、死亡占比。
- `damage`：对英雄伤害、伤害占比、原始伤转、修正伤转、队内/全场伤害第一。
- `gold`：金币、经济占比、经济第一。
- `mitigation`：承伤值、自我减免、实际承受、承伤占比、每死承伤、承伤第一。
- `healing`：总治疗、队友治疗、治疗占比、治疗第一。
- `shielding`：队友护盾。
- `control`：控制次数、分均控制、控制占比、控杀、控杀占比、控制转化和控制时长。

### `score`

`score.value` 是最终正式分；`score.evaluation` 是分数评价；`score.tag` 是评分函数选出的单个行为标签（保留为兼容字段）。评分称号和娱乐称号会合并进入称号池，相关字段含义：

| 字段 | 含义 |
| --- | --- |
| `title` | 当前随机展示的一个合并称号 |
| `titles` | 本局全部命中的评分称号和娱乐称号 |
| `titleDetails` | 每个称号的层级、类别（评分/通用/定位/英雄/对局）和简短理由 |
| `factTags` | 伤害第一、评分第一等可验证事实 |

称号不会改变分数，也不会进入画像统计；展示随机只改变 `title`，同一次响应里的全部结果保持一致。规则见[称号系统](title-system.md)。

`score.parts` 是实际构成正式分的子项。辅助启用全服基准时，旧参团/人头质量/有效伤害小项被全服分替换，`parts.globalBaseline` 显示 `0-25` 的全服分；悬浮提示只突出这一行全服得分，不要求调用方展示全部全服中间计算。

`score.matchContext` 是碾压局诊断：是否启用、K/D/伤害/推塔门控、败方首塔时间、KDA 借权、校准变化和败方抵抗保护。败方推两座塔时会明确记录为不属于碾压局。

### `match.participants` 和 `comparison`

当历史缓存带有完整同局快照时，接口会用软件当前正式评分重新计算所有参与者：

```json
{
  "participants": [
    {
      "participantId": 1,
      "gameName": "玩家",
      "tagLine": "1234",
      "owner": true,
      "champion": {},
      "score": 72,
      "baseScore": 72,
      "globalScore": null,
      "evaluation": "小有实力",
      "tag": "稳定火力",
      "role": {},
      "metrics": {}
    }
  ],
  "comparison": {
    "participantCount": 10,
    "teamCount": 5,
    "teammateCount": 4,
    "opponentCount": 5,
    "ownScore": 72,
    "teammatesAverageScore": 64.5,
    "opponentAverageScore": 68.2,
    "teamScoreRank": 1,
    "gameScoreRank": 3,
    "teamScoreLeader": true,
    "gameScoreLeader": false,
    "teamScoreGapToTeammates": 7.5,
    "scoreGapToOpponents": 3.8
  }
}
```

`globalScore` 只有该参与者属于辅助且全服基准可用时才有值。旧缓存或单人残缺对局可能没有参与者和相对均值；缺失值返回 `null` 或省略，不会把队友当作 0 分。

## 错误响应

```json
{
  "ok": false,
  "error": {
    "code": "query_failed",
    "message": "具体原因"
  }
}
```

常见错误码：

| HTTP | code | 含义 |
| ---: | --- | --- |
| 400 | `invalid_request` | 参数缺失、格式无效或超出范围 |
| 403 | `browser_request_rejected` | 请求带网页 `Origin` |
| 404 | `not_found` | 路径不存在 |
| 405 | `method_not_allowed` | 只支持 GET |
| 409 | `request_in_progress` | 同一时间已有一个接口请求 |
| 502 | `query_failed` | 上游战绩或前端计算失败 |
| 503 | `frontend_not_ready` | 软件尚未完成初始化 |
| 504 | `request_timeout` | 等待软件计算超过 180 秒 |

接口同一时间只执行一个历史请求。重复请求应等待前一个请求完成，而不是并发重试。

## 安全边界

- 只监听 `127.0.0.1`。
- 不返回 SGP 原始响应、认证 token 或客户端端口信息。
- 不设置允许跨域的响应头，并主动拒绝带 `Origin` 的战绩请求。
- 查询参数长度、字符和场数都有上限。

## 相关文档

- [评分系统](scoring-system.md)
- [全服基准](global-baselines.md)
- [称号系统](title-system.md)
- [校准记录](calibration.md)
