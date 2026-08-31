# 本机数据接口

软件启动并完成页面初始化后，会在本机监听：

```text
http://127.0.0.1:17390
```

接口只绑定回环地址，不会暴露到局域网或公网；同时不提供 CORS 响应头，网页跨域请求会被拒绝。适合本机脚本、自动化工具或本地数据看板调用。

## 健康检查

```text
GET /health
```

返回 `ready: true` 时，前端评分桥接已就绪。

## 玩家模式战绩

```text
GET /v1/player-history?gameName={游戏ID}&tagLine={编号}&server={区服}&mode={模式}&count={场数}
```

示例：

```powershell
Invoke-RestMethod 'http://127.0.0.1:17390/v1/player-history?gameName=%E7%8E%A9%E5%AE%B6&tagLine=12345&server=TENCENT_HN1&mode=hex-aram&count=50'
```

参数：

- `gameName`：游戏 ID，与 `tagLine` 分开传入。
- `tagLine`：编号，例如 `12345`。
- `server`：具体国服区服，例如 `TENCENT_HN1`。不支持 `ALL`。
- `mode`：`hex-aram`、`aram`、`ranked`、`normal`、`arena` 或 `all`。也接受 `2400`、`450`、`海克斯大乱斗`、`大乱斗`、`排位`、`匹配` 等常用别名。
- `count`：目标场数，范围 `1` 到 `1000`。

接口会持续扫描最近战绩，直到凑齐对应模式的目标场数，或确认没有更多可用历史，或扫描到软件当前的 1000 场上限。这里的 `depthLoaded` 是后端过滤 PVE、无效短局之后的可用对局数，并不等于原始历史扫描深度；接口会在模式对局稀疏时自动继续加深扫描。`coverage.scannedGames` 表示本次已经拿来分析的可用历史数量，`coverage.complete` 为 `false` 时，说明该模式的可用历史不足目标场数。

## 返回内容

成功响应的顶层结构为：

```json
{
  "ok": true,
  "data": {
    "apiVersion": 1,
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

`globalBaseline` 是本次响应使用的全服基准快照摘要。它包含数据集版本、来源、校验后的英雄数量、根节点 `globalAverages`（当前快照反算出的全服人均 21 项数据）以及指标说明。全服快照仍然是英雄均值，不包含中位数或玩家百分位，因此接口中的“全服倍率”只能表示相对该英雄均值的表现倍数。

`games` 中每局包含软件展示与评分所需的数据：基础对局信息、英雄、K/D/A、伤害/经济/承伤/治疗/护盾/控制数值与占比、得分、评价、定位、评分小项、碾压修正和全服基准结果。`score.title` 是本局随机展示的一个娱乐称号，`score.titles` 是本局所有命中的称号，`score.titleDetails` 包含称号层级、类别和简短判定理由，`score.factTags` 是“伤害全场第一”“评分队内第一”等事实标签。

称号规则以普通玩家和普通玩家偏上样本为校准目标：常规称号使用评分区间、职责指标和同局相对位置，极端低分或极端队友均值只用于少数调侃称号。普通玩家的“队友很差”主要使用约 `60` 分的可见区间；`45` 分以下只作为极少数极端条件，不会把常规称号锁死在几乎不存在的样本上。评分 `20` 分以下也只保留给明显挂机或残缺数据一类的极端情况。

称号命中后会保留全部结果，但展示名由称号池随机抽取：同一份接口响应内保持稳定，下一次刷新接口会重新抽取，重新打开软件也会重新抽取。这样战绩不变时仍会有新的展示称号，而悬浮窗中的全部命中称号不会丢失。

### 普通玩家校准样本

称号条件用三组实际海克斯大乱斗样本（每组目标 100 局）做校准：普通玩家、普通玩家偏上和明显高水平玩家。接口会因为模式筛选继续向前扫描，因此三组样本都能返回 100 局；样本中位数/均值会随账号和数据快照变化，下面只记录本轮校准时的量级：

| 样本 | 平均分 | 中位数 | “一般人机”命中 | “小泯一把”命中 | “本质高手”命中 | “悲情英雄”命中 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 普通玩家 | 约 65 | 约 65 | 18/100 | 17/100 | 17/100 | 18/100 |
| 普通玩家偏上 | 约 71 | 约 78 | 10/100 | 6/100 | 24/100 | 26/100 |
| 高水平玩家 | 约 89 | 约 94 | 7/100 | 2/100 | 70/100 | 31/100 |

这组分布体现了两个原则：普通玩家打出正常或亮眼的一局应当能得到称号；“本质高手”会随表现显著增加，但并不要求达到 95 分或全场第一。相反，队友平均分不超过 `45` 以及评分不超过 `20` 在普通样本中几乎只代表极端局（通常是挂机、残缺数据或极端碾压），只用于少数调侃称号，不作为常规夸奖称号的入口。

当历史缓存包含完整同局快照时，每局还会返回：

```json
{
  "match": {
    "participants": [
      {
        "participantId": 1,
        "gameName": "玩家",
        "tagLine": "1234",
        "owner": false,
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
}
```

`participants` 中的 `score`、`role` 和 `metrics` 均由软件当前评分函数重新计算，不是 SGP 原始分数；`globalScore` 只有该参与者属于辅助且全服基准可用时才有值。`comparison` 的均值和名次只在能识别完整队伍时才有意义。旧缓存、接口返回的单人残缺对局或缺少同局快照时，`participants` 可以为空，相关均值会返回 `null`，不会把缺失的队友当成 0 分。

接口不会返回 SGP 原始 `Game` 对象、原始参与者数组或客户端鉴权信息。`profile` 和 `championProfiles` 使用软件当前同一套时间加权、称号和评分算法计算，因此与数据统计页保持一致。

失败时会返回：

```json
{
  "ok": false,
  "error": {
    "code": "query_failed",
    "message": "具体原因"
  }
}
```

同一时间只执行一个本机接口请求，重复请求会返回 `409 request_in_progress`，避免并发拉取战绩影响客户端和软件页面。
