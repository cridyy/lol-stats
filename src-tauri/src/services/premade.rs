use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use super::models::{LivePremadeMarker, LiveTeam, RecentGame};

pub const HISTORY_PREMADE_THRESHOLD: usize = 3;

const PREMADE_GROUP_IDS: [&str; 10] = ["A", "B", "C", "D", "E", "F", "G", "H", "I", "J"];
const EMPTY_PUUID: &str = "00000000-0000-0000-0000-000000000000";

/// 组队识别所需的最小历史证据。
///
/// 实时战绩和详情页都先把各自的数据整理为这个结构，再走同一个分组算法，
/// 避免两处因为阈值或候选排序不同而显示出不一致的结果。
#[derive(Clone, Debug, Default)]
pub struct PremadePlayerEvidence {
    pub puuid: String,
    pub explicit_group_id: Option<u32>,
    pub recent_games: Vec<RecentGame>,
}

#[derive(Clone, Debug, Default)]
pub struct PremadeTeamEvidence {
    pub players: Vec<PremadePlayerEvidence>,
}

/// 给实时战绩玩家写入统一的组队标记。
pub fn assign_live_premade_markers(teams: &mut [LiveTeam]) {
    let evidence = teams
        .iter()
        .map(|team| PremadeTeamEvidence {
            players: team
                .players
                .iter()
                .map(|player| PremadePlayerEvidence {
                    puuid: player.puuid.clone(),
                    explicit_group_id: player.team_participant_id,
                    recent_games: player
                        .stats
                        .as_ref()
                        .map(|stats| stats.recent_games.clone())
                        .unwrap_or_default(),
                })
                .collect(),
        })
        .collect::<Vec<_>>();
    let markers = detect_premade_markers(&evidence);

    for player in teams.iter_mut().flat_map(|team| team.players.iter_mut()) {
        player.premade = markers.get(&normalize_puuid(&player.puuid)).cloned();
    }
}

/// 根据当前对局各队玩家的历史同队证据，生成每个玩家的组队标记。
pub fn detect_premade_markers(teams: &[PremadeTeamEvidence]) -> HashMap<String, LivePremadeMarker> {
    let mut markers = HashMap::<String, LivePremadeMarker>::new();
    let mut marked = HashSet::<String>::new();
    let mut group_index = 0usize;

    // 客户端直接提供的分组信息优先级最高。
    for team in teams {
        for group in explicit_premade_groups(team) {
            assign_premade_group(&mut markers, group, &mut marked, &mut group_index);
        }
    }

    // 其余玩家再用历史同队关系推断，顺序与实时战绩保持一致。
    for team in teams {
        for group in inferred_premade_groups(team, &marked) {
            assign_premade_group(&mut markers, group, &mut marked, &mut group_index);
        }
    }

    markers
}

#[derive(Clone)]
struct PremadeCandidate {
    puuids: Vec<String>,
    source: &'static str,
    together_times: usize,
}

fn explicit_premade_groups(team: &PremadeTeamEvidence) -> Vec<PremadeCandidate> {
    let mut groups = BTreeMap::<u32, Vec<String>>::new();

    for player in &team.players {
        let Some(group_id) = player.explicit_group_id.filter(|id| *id != 0) else {
            continue;
        };

        if !is_real_puuid(&player.puuid) {
            continue;
        }

        groups
            .entry(group_id)
            .or_default()
            .push(normalize_puuid(&player.puuid));
    }

    groups
        .into_values()
        .map(|mut puuids| {
            puuids.sort();
            puuids.dedup();
            PremadeCandidate {
                puuids,
                source: "客户端分组",
                together_times: 0,
            }
        })
        .filter(|candidate| candidate.puuids.len() >= 2)
        .collect()
}

fn inferred_premade_groups(
    team: &PremadeTeamEvidence,
    marked: &HashSet<String>,
) -> Vec<PremadeCandidate> {
    let current_players = team
        .players
        .iter()
        .filter(|player| is_real_puuid(&player.puuid))
        .map(|player| normalize_puuid(&player.puuid))
        .collect::<BTreeSet<_>>();

    if current_players.len() < 2 || current_players.len() > 10 {
        return Vec::new();
    }

    let current_players = current_players
        .into_iter()
        .filter(|puuid| !marked.contains(puuid))
        .collect::<Vec<_>>();

    if current_players.len() < 2 {
        return Vec::new();
    }

    let current_set = current_players.iter().cloned().collect::<HashSet<_>>();
    let mut pair_games = HashMap::<(String, String), HashSet<u64>>::new();

    for player in &team.players {
        for game in &player.recent_games {
            let same_side = game
                .team_puuids
                .iter()
                .map(|puuid| normalize_puuid(puuid))
                .filter(|puuid| current_set.contains(puuid) && !marked.contains(puuid))
                .collect::<BTreeSet<_>>();

            if same_side.len() < 2 {
                continue;
            }

            let same_side = same_side.into_iter().collect::<Vec<_>>();
            for i in 0..same_side.len().saturating_sub(1) {
                for j in (i + 1)..same_side.len() {
                    pair_games
                        .entry((same_side[i].clone(), same_side[j].clone()))
                        .or_default()
                        .insert(game.game_id);
                }
            }
        }
    }

    let mut candidates = Vec::<PremadeCandidate>::new();
    let total_masks = 1usize << current_players.len();
    for mask in 0..total_masks {
        if mask.count_ones() < 2 {
            continue;
        }

        let puuids = current_players
            .iter()
            .enumerate()
            .filter_map(|(index, puuid)| ((mask & (1usize << index)) != 0).then_some(puuid.clone()))
            .collect::<Vec<_>>();

        let Some(together_times) = premade_subset_times(&puuids, &pair_games) else {
            continue;
        };

        if together_times >= HISTORY_PREMADE_THRESHOLD {
            candidates.push(PremadeCandidate {
                puuids,
                source: "历史同队",
                together_times,
            });
        }
    }

    candidates.sort_by(|left, right| {
        right
            .puuids
            .len()
            .cmp(&left.puuids.len())
            .then_with(|| right.together_times.cmp(&left.together_times))
            .then_with(|| left.puuids.cmp(&right.puuids))
    });

    let mut selected = Vec::<PremadeCandidate>::new();
    let mut used = HashSet::<String>::new();
    for candidate in candidates {
        if candidate.puuids.iter().any(|puuid| used.contains(puuid)) {
            continue;
        }

        for puuid in &candidate.puuids {
            used.insert(puuid.clone());
        }
        selected.push(candidate);
    }

    selected
}

fn premade_subset_times(
    puuids: &[String],
    pair_games: &HashMap<(String, String), HashSet<u64>>,
) -> Option<usize> {
    let mut min_times = usize::MAX;

    for i in 0..puuids.len().saturating_sub(1) {
        for j in (i + 1)..puuids.len() {
            let key = if puuids[i] <= puuids[j] {
                (puuids[i].clone(), puuids[j].clone())
            } else {
                (puuids[j].clone(), puuids[i].clone())
            };
            let times = pair_games.get(&key)?.len();
            min_times = min_times.min(times);
        }
    }

    Some(min_times)
}

fn assign_premade_group(
    markers: &mut HashMap<String, LivePremadeMarker>,
    group: PremadeCandidate,
    marked: &mut HashSet<String>,
    group_index: &mut usize,
) -> bool {
    if group.puuids.len() < 2 || group.puuids.iter().any(|puuid| marked.contains(puuid)) {
        return false;
    }

    let group_id = PREMADE_GROUP_IDS
        .get(*group_index)
        .copied()
        .unwrap_or("Z")
        .to_string();
    let marker = LivePremadeMarker {
        label: format!("{}黑", group.puuids.len()),
        group_id,
        source: group.source.to_string(),
        together_times: group.together_times,
        member_puuids: group.puuids.clone(),
    };

    for puuid in &group.puuids {
        markers.insert(puuid.clone(), marker.clone());
        marked.insert(puuid.clone());
    }
    *group_index += 1;
    true
}

fn is_real_puuid(puuid: &str) -> bool {
    let normalized = puuid.trim();
    !normalized.is_empty() && !normalized.eq_ignore_ascii_case(EMPTY_PUUID)
}

fn normalize_puuid(puuid: &str) -> String {
    puuid.trim().to_ascii_lowercase()
}
