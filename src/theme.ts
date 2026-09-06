import { disable, enable } from "darkreader"

const DARK_THEME = {
  mode: 1 as const,
  brightness: 100,
  contrast: 100,
  grayscale: 0,
  sepia: 0,
  darkSchemeBackgroundColor: "#1f1f1f",
  darkSchemeTextColor: "#e8e8e8",
  scrollbarColor: "#5b5b5b",
  selectionColor: "#666666",
  styleSystemControls: true,
}

const THEME_STYLE_ID = "lol-stats-dark-theme"

// Keep the app shell neutral while leaving score, result, rank and status colors intact.
const DARK_THEME_CSS = `
html[data-theme="dark"],
html[data-theme="dark"] body {
  background: #1f1f1f !important;
  color: #e8e8e8 !important;
}

html[data-theme="dark"] .app-shell {
  background: #1f1f1f !important;
  color: #e8e8e8 !important;
}

html[data-theme="dark"] .sidebar {
  border-right-color: #353535 !important;
  background: #181818 !important;
}

html[data-theme="dark"] .workspace {
  background: #1f1f1f !important;
}

html[data-theme="dark"] .app-shell :is(
  .drill-tabs button,
  .details-empty,
  .candidate-panel,
  .candidate-item,
  .history,
  .history-item,
  .search-box,
  .metric,
  .panel,
  .profile-tags,
  .ability-item,
  .role-bar-row,
  .player-card,
  .recent-header,
  .match-detail-team,
  .match-detail-toolbar,
  .match-detail-modal,
  .tool-row,
  .chat-row,
  .aramkit-tool,
  .tier-row,
  .champion-pool,
  .designer-editor,
  .designer-preview,
  .fun-group,
  .equipment-result,
  .augment-result
) {
  background-color: #252525 !important;
  color: #e1e1e1 !important;
  border-color: #3b3b3b !important;
}

html[data-theme="dark"] .app-shell :is(
  input,
  select,
  textarea,
  .settings-mode-options,
  .signature-input,
  .filter-control,
  .ranking-switch select,
  .icon-search,
  .search-box
) {
  background-color: #202020 !important;
  color: #e5e5e5 !important;
  border-color: #474747 !important;
}

html[data-theme="dark"] .side-nav button {
  color: #b4b4b4 !important;
  background: transparent !important;
}

html[data-theme="dark"] .side-nav button.active,
html[data-theme="dark"] .side-nav .nav-history button,
html[data-theme="dark"] .theme-entry,
html[data-theme="dark"] .settings-entry {
  color: #f0f0f0 !important;
  background: #343434 !important;
}

html[data-theme="dark"] .side-nav button:hover,
html[data-theme="dark"] .side-nav .nav-history button:not(:disabled):hover,
html[data-theme="dark"] .theme-entry:hover,
html[data-theme="dark"] .settings-entry:hover,
html[data-theme="dark"] .candidate-item:hover,
html[data-theme="dark"] .drill-tabs button:hover {
  background: #3d3d3d !important;
}

html[data-theme="dark"] .app-shell :is(
  .record-row,
  .recent-game
) {
  background-color: #252525 !important;
  border-color: #3f3f3f !important;
}

/* Results are status-bearing: a win is green and a loss is red. */
html[data-theme="dark"] .app-shell .record-row.win,
html[data-theme="dark"] .app-shell .recent-game.win,
html[data-theme="dark"] .app-shell .match-detail-team-header.win {
  background-color: #1f3929 !important;
  border-color: #599c72 !important;
}

html[data-theme="dark"] .app-shell .record-row.lose,
html[data-theme="dark"] .app-shell .recent-game.loss,
html[data-theme="dark"] .app-shell .match-detail-team-header.lose {
  background-color: #382326 !important;
  border-color: #ca4b4b !important;
}

/* Keep score and profile tiers colorful and readable in the dark palette. */
html[data-theme="dark"] .app-shell :is(.score-excellent, .profile-excellent, .profile-tier-apex) {
  color: #ffebb0 !important;
  border-color: #d9a731 !important;
  background: linear-gradient(135deg, #76560c, #9c7215 48%, #71500b) !important;
  opacity: 1 !important;
}

html[data-theme="dark"] .app-shell :is(.score-good, .profile-good, .profile-tier-steady) {
  color: #c4f8d7 !important;
  border-color: #4caf75 !important;
  background: #1b603c !important;
  opacity: 1 !important;
}

html[data-theme="dark"] .app-shell :is(.score-average, .profile-average, .profile-tier-normal) {
  color: #d3e6ff !important;
  border-color: #548fd1 !important;
  background: #1d4f85 !important;
  opacity: 1 !important;
}

html[data-theme="dark"] .app-shell :is(
  .score-poor,
  .profile-poor,
  .profile-tier-big-pit
) {
  color: #ffd6d6 !important;
  border-color: #d95c5c !important;
  background: #7a363b !important;
  opacity: 1 !important;
}

html[data-theme="dark"] .app-shell .profile-tier-small-pit {
  color: #ffe0a6 !important;
  border-color: #d69a42 !important;
  background: #754618 !important;
  opacity: 1 !important;
}

html[data-theme="dark"] .app-shell :is(.tag-strong, .tag-band-excellent) {
  color: #ffebb0 !important;
  background: #70520d !important;
  border-color: #d9a731 !important;
  opacity: 1 !important;
}

html[data-theme="dark"] .app-shell :is(.tag-team, .tag-band-normal) {
  color: #c4f8d7 !important;
  background: #1b603c !important;
  border-color: #4caf75 !important;
  opacity: 1 !important;
}

html[data-theme="dark"] .app-shell .tag-warn {
  color: #ffe0a6 !important;
  background: #754618 !important;
  border-color: #d69a42 !important;
  opacity: 1 !important;
}

html[data-theme="dark"] .app-shell .tag-neutral {
  color: #c7f2ea !important;
  background: #285c55 !important;
  border-color: #52a99d !important;
  opacity: 1 !important;
}

html[data-theme="dark"] .app-shell :is(.tag-danger, .tag-band-disaster) {
  color: #ffd6d6 !important;
  background: #7a363b !important;
  border-color: #d95c5c !important;
  opacity: 1 !important;
}

html[data-theme="dark"] .app-shell :is(.tag-band-low, .mobile-chip.tag-band-low) {
  color: #d3e6ff !important;
  background: #1d4f85 !important;
  border-color: #548fd1 !important;
  opacity: 1 !important;
}

html[data-theme="dark"] .app-shell :is(.tone-good, .stat-share b) {
  color: #59d99b !important;
}

html[data-theme="dark"] .app-shell .tone-warn {
  color: #f0b45e !important;
}

html[data-theme="dark"] .app-shell :is(
  .tone-bad,
  .stat-cell strong.leader,
  .stat-share.leader,
  .stat-share.leader b,
  .profile-stat-row b,
  .profile-carry-row b
) {
  color: #ff7777 !important;
}

html[data-theme="dark"] .app-shell .player-name-button {
  color: #75baff !important;
}

html[data-theme="dark"] .app-shell :is(
  .record-row.win .queue-name,
  .match-detail-team-header.win
) {
  color: #a8eabc !important;
}

html[data-theme="dark"] .app-shell :is(
  .record-row.lose .queue-name,
  .match-detail-team-header.lose
) {
  color: #ff9696 !important;
}

html[data-theme="dark"] .app-shell :is(
  .eyebrow,
  .app-version,
  .details-empty span,
  .empty-subtitle,
  .candidate-title span,
  .candidate-item em,
  .history-title,
  .time-cell span,
  .time-cell em,
  .queue-name,
  .kda-cell span,
  .stat-cell span,
  .identity-text > span,
  .team-title span,
  .average-metrics i,
  .profile-heading span,
  .ability-item div span,
  .ability-item footer span,
  .match-detail-toolbar-main span,
  .match-detail-state,
  .metric span,
  .metric small,
  .tool-title span,
  .external-url
) {
  color: #999999 !important;
}

/* Compact live cards use their own labels instead of the full record-row cells. */
html[data-theme="dark"] .app-shell :is(
  .identity-context small,
  .rank-level i,
  .rank-entry i,
  .rank-win-rate i,
  .rank-kda i,
  .profile-metrics-row i,
  .recent-kda span
) {
  color: #a5a5a5 !important;
}

html[data-theme="dark"] .app-shell :is(
  .identity-context,
  .rank-line,
  .profile-metrics-row,
  .recent-kda
) {
  color: #a5a5a5 !important;
}

html[data-theme="dark"] .app-shell :is(
  .rank-level b,
  .rank-entry strong,
  .rank-win-rate b,
  .rank-kda b,
  .profile-metrics-row b,
  .recent-kda strong
) {
  color: #e1e1e1 !important;
}

html[data-theme="dark"] .app-shell .rank-level b {
  color: #ff7777 !important;
}

html[data-theme="dark"] .app-shell .rank-entry strong {
  color: #59d99b !important;
}

html[data-theme="dark"] .app-shell :is(.rank-win-rate, .rank-kda) b.tone-good {
  color: #59d99b !important;
}

html[data-theme="dark"] .app-shell :is(.rank-win-rate, .rank-kda) b.tone-warn {
  color: #f0b45e !important;
}

html[data-theme="dark"] .app-shell :is(.rank-win-rate, .rank-kda) b.tone-bad {
  color: #ff7777 !important;
}

html[data-theme="dark"] .app-shell .rank-line {
  border-color: #444444 !important;
}

html[data-theme="dark"] .app-shell .rank-divider {
  background-color: #444444 !important;
}

html[data-theme="dark"] .app-shell .player-avatar {
  border-color: #555555 !important;
}

html[data-theme="dark"] .app-shell .player-error {
  color: #ffb0b0 !important;
  background: #4a292d !important;
}

html[data-theme="dark"] .app-shell .role-bar-track {
  background: #3b3b3b !important;
}

html[data-theme="dark"] .app-shell :is(.switch-control, .live-feature-switch i) {
  background: #4a4a4a !important;
}

html[data-theme="dark"] .app-shell .switch-control.active,
html[data-theme="dark"] .app-shell .live-feature-switch.active i,
html[data-theme="dark"] .app-shell .availability-option.active {
  background: #238b65 !important;
}

html[data-theme="dark"] .app-shell .live-feature-switch.active {
  color: #78e0b0 !important;
  border-color: #328f6d !important;
  background: #18382c !important;
}

html[data-theme="dark"] .app-shell .toast-card {
  color: #e1e1e1 !important;
  background: #252525 !important;
  border-color: #464646 !important;
}

html[data-theme="dark"] .app-shell .toast-card.info {
  border-left-color: #2f78d6 !important;
}

html[data-theme="dark"] .app-shell .toast-card.success {
  border-left-color: #2f9d70 !important;
}

html[data-theme="dark"] .app-shell .toast-card.warning {
  border-left-color: #d69b2f !important;
}

html[data-theme="dark"] .app-shell .toast-card.error {
  border-left-color: #c94d4b !important;
}

html[data-theme="dark"] .app-shell :is(
  .release-notes-modal,
  .update-modal,
  .center-dialog
) {
  color: #e1e1e1 !important;
  background: #242424 !important;
  border-color: #454545 !important;
}

html[data-theme="dark"] .app-shell :is(
  .update-notes,
  .release-notes-modal li,
  .center-dialog p,
  .update-modal dl div
) {
  background: #2d2d2d !important;
  color: #d4d4d4 !important;
}

html[data-theme="dark"] .settings-entry.active {
  color: #ffffff !important;
  background: #4b4b4b !important;
}

html[data-theme="dark"] .app-shell .settings-section {
  color: #d4d4d4 !important;
  border-color: #3b3b3b !important;
}

html[data-theme="dark"] .app-shell .settings-page :is(
  label > span,
  .settings-detection-mode > span,
  .settings-log-status,
  .settings-section-title span
) {
  color: #b4b4b4 !important;
}

html[data-theme="dark"] .app-shell .settings-page .settings-section-title strong {
  color: #e8e8e8 !important;
}

html[data-theme="dark"] .app-shell :is(
  .progress-overlay,
  .release-notes-overlay,
  .update-overlay,
  .center-overlay,
  .match-detail-overlay
) {
  background: rgba(0, 0, 0, 0.72) !important;
}

html[data-theme="dark"] .app-shell :is(
  .settings-page input[type="range"],
  .role-bar-track
) {
  accent-color: #8b8b8b;
}

html[data-theme="dark"] .app-shell .settings-mode-options button {
  color: #cfcfcf !important;
  background: transparent !important;
}

html[data-theme="dark"] .app-shell .settings-mode-options button[aria-pressed="true"] {
  color: #ffffff !important;
  background: #505050 !important;
}

html[data-theme="dark"] .app-shell .settings-mode-options button:focus-visible {
  outline-color: #bdbdbd !important;
}

html[data-theme="dark"] .app-shell .settings-proxy-actions button:disabled,
html[data-theme="dark"] .app-shell .settings-proxy-address input:disabled,
html[data-theme="dark"] .app-shell .settings-proxy-timeout input:disabled {
  color: #858585 !important;
  background: #292929 !important;
}

html[data-theme="dark"] .app-shell .settings-proxy-result.success {
  color: #a8eabc !important;
}

html[data-theme="dark"] .app-shell .settings-proxy-result.warning {
  color: #e4c078 !important;
}

html[data-theme="dark"] .app-shell .settings-proxy-result.error {
  color: #ff9696 !important;
}

html[data-theme="dark"] .app-shell :is(
  .champion-avatar img,
  .asset-icon img,
  .augment-icon img,
  .hextech-icon img,
  .rank-icon,
  .rank-icon img,
  .rank-card-icon,
  img[src^="asset:"],
  img[src^="data:"]
) {
  filter: none !important;
}

html[data-theme="dark"] ::placeholder {
  color: #777777 !important;
}

/*
 * Navigation, actions and editor chrome are deliberately neutral.  They do
 * not carry match data, so a green or blue surface would only look like a
 * false status signal in dark mode.
 */
html[data-theme="dark"] .app-shell :is(
  .drill-tabs button,
  .record-tabs button,
  .queue-tabs button,
  .queue-picker button,
  .refresh-tab,
  .stats-depth-control,
  .stats-date-trigger,
  .stats-date-actions button,
  .stats-depth-menu button,
  .tool-tabs button,
  .tier-actions button,
  .center-dialog button,
  .designer-actions button,
  .designer-dialog button,
  .rarity-control button,
  .icon-option,
  .friends-actions button,
  .command-button,
  .icon-action,
  .availability-option,
  .match-detail-close,
  .match-detail-analyze,
  .match-detail-tab,
  .chart-toggle,
  .ability-toggle,
  .fun-stats-heading button,
  .data-filter-button,
  .square-picker-button,
  .icon-picker-grid button,
  .settings-actions button,
  .release-notes-secondary,
  .release-notes-primary,
  .update-secondary,
  .update-copy,
  .update-primary,
  .update-close,
  .primary-action,
  .search-box button,
  .return-search,
  .history-open,
  .update-badge
) {
  color: #e4e4e4 !important;
  border-color: #4a4a4a !important;
  background: #303030 !important;
  box-shadow: none !important;
}

html[data-theme="dark"] .app-shell :is(
  .drill-tabs button,
  .record-tabs button,
  .queue-tabs button,
  .queue-picker button,
  .refresh-tab,
  .stats-date-trigger,
  .stats-date-actions button,
  .stats-depth-menu button,
  .tool-tabs button,
  .tier-actions button,
  .center-dialog button,
  .designer-actions button,
  .designer-dialog button,
  .rarity-control button,
  .icon-option,
  .friends-actions button,
  .command-button,
  .icon-action,
  .availability-option,
  .match-detail-close,
  .match-detail-analyze,
  .chart-toggle,
  .ability-toggle,
  .fun-stats-heading button,
  .data-filter-button,
  .square-picker-button,
  .icon-picker-grid button,
  .settings-actions button,
  .release-notes-secondary,
  .release-notes-primary,
  .update-secondary,
  .update-copy,
  .update-primary,
  .update-close,
  .primary-action,
  .search-box button,
  .return-search,
  .history-open
):hover:not(:disabled),
html[data-theme="dark"] .app-shell :is(
  .drill-tabs button.active,
  .record-tabs button.active,
  .queue-tabs button.active,
  .tool-tabs button.active,
  .match-detail-tab.active,
  .icon-option.active,
  .icon-picker-grid button.selected
) {
  color: #f4f4f4 !important;
  border-color: #686868 !important;
  background: #424242 !important;
  box-shadow: inset 0 0 0 1px #5a5a5a !important;
}

html[data-theme="dark"] .app-shell :is(
  .tool-tabs .live-champion-tab,
  .tool-tabs .live-champion-tab.active,
  .designer-preview,
  .designer-editor,
  .saved-design-switch,
  .icon-search,
  .fun-stats-panel,
  .fun-group,
  .fun-row,
  .stats-date-menu
) {
  border-color: #414141 !important;
  background: #242424 !important;
  box-shadow: none !important;
}

html[data-theme="dark"] .app-shell .designer-preview {
  background-image: none !important;
  background-color: #202020 !important;
}

html[data-theme="dark"] .app-shell .update-modal::before,
html[data-theme="dark"] .app-shell .release-notes-modal li::before {
  background: #666666 !important;
}

/* The progress container must stay transparent; only the circular ring is visible. */
html[data-theme="dark"] .app-shell .progress-modal {
  background: transparent !important;
  border: 0 !important;
  box-shadow: none !important;
}

html[data-theme="dark"] .app-shell .progress-ring {
  border-radius: 50% !important;
  background:
    radial-gradient(circle at center, #252525 0 58%, transparent 59%),
    conic-gradient(#a3a3a3 var(--progress), #3a3a3a 0deg) !important;
  box-shadow: 0 20px 50px rgb(0 0 0 / 42%) !important;
}

html[data-theme="dark"] .app-shell .progress-ring :is(div, strong, span) {
  color: #f2f2f2 !important;
}

html[data-theme="dark"] .app-shell :is(
  .settings-page input[type="checkbox"],
  .stats-toggle input,
  .queue-picker input
) {
  accent-color: #8a8a8a !important;
}

/* Restore only elements whose color has an actual product or data meaning. */
html[data-theme="dark"] .app-shell :is(
  .live-feature-switch.active,
  .availability-option.active,
  .switch-control.active
) {
  border-color: #328f6d !important;
  color: #e7fff3 !important;
  background: #1d6f55 !important;
}

html[data-theme="dark"] .app-shell .live-feature-switch.active i {
  background: #32a97c !important;
}

html[data-theme="dark"] .app-shell .rarity-control button.silver.active {
  border-color: #98a9ad !important;
  color: #edf4f5 !important;
  background: #465255 !important;
}

html[data-theme="dark"] .app-shell .rarity-control button.gold.active {
  border-color: #c79a3c !important;
  color: #ffe5a3 !important;
  background: #60470e !important;
}

html[data-theme="dark"] .app-shell .rarity-control button.prismatic.active {
  border-color: #a78cda !important;
  color: #eadfff !important;
  background: #44345f !important;
}

html[data-theme="dark"] .app-shell :is(
  .augment-picker-button.silver,
  .icon-picker-grid button.silver
) {
  border-color: #7b8c9e !important;
  background: #596a7b !important;
}

html[data-theme="dark"] .app-shell :is(
  .augment-picker-button.gold,
  .icon-picker-grid button.gold
) {
  border-color: #c49022 !important;
  background: #805a11 !important;
}

html[data-theme="dark"] .app-shell :is(
  .augment-picker-button.prismatic,
  .icon-picker-grid button.prismatic
) {
  border-color: #8c5cc4 !important;
  background: #5b3d81 !important;
}

html[data-theme="dark"] .app-shell :is(
  .augment-picker-button.bronze,
  .icon-picker-grid button.bronze
) {
  border-color: #a56d4e !important;
  background: #654131 !important;
}

html[data-theme="dark"] .app-shell :is(
  .progress-modal button,
  .history-delete,
  .center-dialog .danger,
  .designer-dialog .danger
) {
  color: #ffe4e4 !important;
  border-color: #ba5151 !important;
  background: #7b3636 !important;
}

/* Rich, opaque status chips. These are data signals rather than page chrome. */
html[data-theme="dark"] .app-shell :is(
  .score-excellent,
  .score-good,
  .score-average,
  .score-poor,
  .profile-excellent,
  .profile-good,
  .profile-average,
  .profile-poor,
  .profile-tier-apex,
  .profile-tier-steady,
  .profile-tier-normal,
  .profile-tier-small-pit,
  .profile-tier-big-pit,
  .champion-label-tag,
  .accolade-tag,
  .text-grid .augment-tag,
  .mobile-runes.text-runes :is(.augment-prismatic, .augment-gold, .augment-silver, .augment-bronze),
  .augment-result
) {
  opacity: 1 !important;
}

html[data-theme="dark"] .app-shell .accolade-tag.damage-global {
  color: #fff6e6 !important;
  background: linear-gradient(135deg, #d9372d, #ff8a2a) !important;
}

html[data-theme="dark"] .app-shell .accolade-tag.damage-team {
  color: #fff4df !important;
  background: #d96b20 !important;
}

html[data-theme="dark"] .app-shell .accolade-tag.mitigation {
  color: #e5fff0 !important;
  background: #23875b !important;
}

html[data-theme="dark"] .app-shell .accolade-tag.healing {
  color: #e5fff0 !important;
  background: #259a66 !important;
}

html[data-theme="dark"] .app-shell .accolade-tag.conversion {
  color: #fff0f0 !important;
  background: #bd3b49 !important;
}

html[data-theme="dark"] .app-shell .accolade-tag.gold {
  color: #3f2900 !important;
  background: #f1b739 !important;
}

html[data-theme="dark"] .app-shell .text-grid .augment-tag {
  border-color: #555555 !important;
  color: #eeeeee !important;
  background: #353535 !important;
}

html[data-theme="dark"] .app-shell :is(
  .text-grid .augment-prismatic,
  .mobile-runes.text-runes .augment-prismatic
) {
  border-color: #a56bd1 !important;
  color: #f1dcff !important;
  background: #593779 !important;
}

html[data-theme="dark"] .app-shell :is(
  .text-grid .augment-gold,
  .mobile-runes.text-runes .augment-gold
) {
  border-color: #d4a23c !important;
  color: #ffe7a9 !important;
  background: #6b4b0c !important;
}

html[data-theme="dark"] .app-shell :is(
  .text-grid .augment-silver,
  .mobile-runes.text-runes .augment-silver
) {
  border-color: #92a7b8 !important;
  color: #e4f1fb !important;
  background: #40515d !important;
}

html[data-theme="dark"] .app-shell :is(
  .text-grid .augment-bronze,
  .mobile-runes.text-runes .augment-bronze
) {
  border-color: #bd8060 !important;
  color: #ffe0cb !important;
  background: #633d2c !important;
}

html[data-theme="dark"] .app-shell .augment-result.silver {
  border-color: #92a7b8 !important;
  border-left-color: #b2c6d5 !important;
  background: #40515d !important;
}

html[data-theme="dark"] .app-shell .augment-result.gold {
  border-color: #d4a23c !important;
  border-left-color: #f1c14e !important;
  background: #6b4b0c !important;
}

html[data-theme="dark"] .app-shell .augment-result.prismatic {
  border-color: #a56bd1 !important;
  border-left-color: #ca9ae9 !important;
  background: #593779 !important;
}

html[data-theme="dark"] .app-shell .augment-result.bronze {
  border-color: #bd8060 !important;
  border-left-color: #dda17f !important;
  background: #633d2c !important;
}

html[data-theme="dark"] .app-shell .augment-result :is(strong, dd) {
  color: #fff7e8 !important;
}

html[data-theme="dark"] .app-shell .augment-result :is(span, dt) {
  color: #e4e4e4 !important;
}

html[data-theme="dark"] .app-shell :is(
  .augment-result.silver .asset-icon,
  .augment-picker-button.silver .asset-icon,
  .icon-picker-grid button.silver .asset-icon
) {
  border-color: #b2c6d5 !important;
  background: #657789 !important;
}

html[data-theme="dark"] .app-shell :is(
  .augment-result.gold .asset-icon,
  .augment-picker-button.gold .asset-icon,
  .icon-picker-grid button.gold .asset-icon
) {
  border-color: #f1c14e !important;
  background: #8f640f !important;
}

html[data-theme="dark"] .app-shell :is(
  .augment-result.prismatic .asset-icon,
  .augment-picker-button.prismatic .asset-icon,
  .icon-picker-grid button.prismatic .asset-icon
) {
  border-color: #ca9ae9 !important;
  background: #613b89 !important;
}

html[data-theme="dark"] .app-shell :is(
  .augment-result.bronze .asset-icon,
  .augment-picker-button.bronze .asset-icon,
  .icon-picker-grid button.bronze .asset-icon
) {
  border-color: #dda17f !important;
  background: #734631 !important;
}

html[data-theme="dark"] .app-shell :is(
  .augment-table-icon.augment-prismatic,
  .asset-tag.augment-prismatic
) {
  border-color: #a56bd1 !important;
  color: #f1dcff !important;
  background: #593779 !important;
}

html[data-theme="dark"] .app-shell :is(
  .augment-table-icon.augment-gold,
  .asset-tag.augment-gold
) {
  border-color: #d4a23c !important;
  color: #ffe7a9 !important;
  background: #6b4b0c !important;
}

html[data-theme="dark"] .app-shell :is(
  .augment-table-icon.augment-silver,
  .asset-tag.augment-silver
) {
  border-color: #92a7b8 !important;
  color: #e4f1fb !important;
  background: #40515d !important;
}

html[data-theme="dark"] .app-shell :is(
  .augment-table-icon.augment-bronze,
  .asset-tag.augment-bronze
) {
  border-color: #bd8060 !important;
  color: #ffe0cb !important;
  background: #633d2c !important;
}

html[data-theme="dark"] .app-shell .augment-rarity-filters button.active.silver {
  border-color: #92a7b8 !important;
  color: #edf6ff !important;
  background: #526574 !important;
}

html[data-theme="dark"] .app-shell .augment-rarity-filters button.active.gold {
  border-color: #d4a23c !important;
  color: #ffe7a9 !important;
  background: #805a11 !important;
}

html[data-theme="dark"] .app-shell .augment-rarity-filters button.active.prismatic {
  border-color: #a56bd1 !important;
  color: #f1dcff !important;
  background: #593779 !important;
}

html[data-theme="dark"] .app-shell :is(
  .team-header.win,
  .chart-row.win
) {
  color: #a8eabc !important;
  border-color: #599c72 !important;
  background: #1f3929 !important;
}

html[data-theme="dark"] .app-shell .chart-row.win .chart-bar {
  background: #52ae73 !important;
}

html[data-theme="dark"] .app-shell :is(
  .team-header.lose,
  .chart-row.lose
) {
  color: #ffb2b2 !important;
  border-color: #ca5e5e !important;
  background: #43272b !important;
}

html[data-theme="dark"] .app-shell .chart-row.lose .chart-bar {
  background: #d96565 !important;
}

html[data-theme="dark"] .app-shell .rank-card {
  border-color: #494949 !important;
  border-left-color: var(--rank-accent) !important;
  background: #2b2b2b !important;
  opacity: 1 !important;
}

/* Premium gold: warm champagne highlights on a restrained bronze-black base. */
html[data-theme="dark"] .app-shell :is(
  .score-excellent,
  .profile-excellent,
  .profile-tier-apex
) {
  color: #f5dfb0 !important;
  border-color: #c7a15d !important;
  background: linear-gradient(135deg, #2d261c 0%, #5a4529 46%, #30271b 100%) !important;
  box-shadow:
    inset 0 0 0 1px rgb(255 226 168 / 18%),
    0 0 18px rgb(190 148 74 / 22%) !important;
}

html[data-theme="dark"] .app-shell :is(
  .tag-strong,
  .tag-band-excellent
) {
  color: #f5dfb0 !important;
  border-color: #b89350 !important;
  background: #493720 !important;
}

html[data-theme="dark"] .app-shell .profile-tier-small-pit {
  color: #f2d29c !important;
  border-color: #ae8443 !important;
  background: #4b3821 !important;
}

html[data-theme="dark"] .app-shell :is(
  .rarity-control button.gold.active,
  .augment-rarity-filters button.active.gold
) {
  border-color: #c7a15d !important;
  color: #f5dfb0 !important;
  background: #403221 !important;
}

html[data-theme="dark"] .app-shell :is(
  .augment-picker-button.gold,
  .icon-picker-grid button.gold
) {
  border-color: #b89350 !important;
  background: #4b3923 !important;
}

html[data-theme="dark"] .app-shell :is(
  .augment-picker-button.gold .asset-icon,
  .icon-picker-grid button.gold .asset-icon,
  .augment-result.gold .asset-icon
) {
  border-color: #d0ad68 !important;
  background: #614a29 !important;
}

html[data-theme="dark"] .app-shell .text-grid .augment-gold,
html[data-theme="dark"] .app-shell .mobile-runes.text-runes .augment-gold {
  border-color: #b89350 !important;
  color: #f5dfb0 !important;
  background: #493720 !important;
}

html[data-theme="dark"] .app-shell .augment-result.gold {
  border-color: #b89350 !important;
  border-left-color: #d0ad68 !important;
  background: #403221 !important;
}

html[data-theme="dark"] .app-shell .augment-table-icon.augment-gold,
html[data-theme="dark"] .app-shell .asset-tag.augment-gold {
  border-color: #b89350 !important;
  color: #f5dfb0 !important;
  background: #493720 !important;
}

html[data-theme="dark"] .app-shell .accolade-tag.gold {
  color: #3b2c17 !important;
  background: #c5a15b !important;
}

/* History is a layout wrapper, not a card. Keep the entry itself transparent. */
html[data-theme="dark"] body .app-shell .search-landing .history,
html[data-theme="dark"] body .app-shell .search-landing .history-item {
  border: 0 !important;
  background-color: transparent !important;
  background-image: none !important;
  box-shadow: none !important;
}

/* The history entry must not inherit the generic dark action-button surface. */
html[data-theme="dark"] body .app-shell .search-landing .history-open {
  border-color: transparent !important;
  background: transparent !important;
  box-shadow: none !important;
  color: #d7d7d7 !important;
}

html[data-theme="dark"] body .app-shell .search-landing .history-open:hover:not(:disabled) {
  border-color: transparent !important;
  background: transparent !important;
  box-shadow: none !important;
}

html[data-theme="dark"] body .app-shell .search-landing .history-open span {
  color: #969696 !important;
}

html[data-theme="dark"] body .app-shell .search-landing .history-open strong {
  color: #e1e1e1 !important;
}

/* Live player cards keep a restrained tier tint on the headline and neutral metrics below. */
html[data-theme="dark"] .app-shell .profile-panel-line {
  gap: 4px !important;
}

html[data-theme="dark"] .app-shell :is(.profile-main-row, .profile-stat-row) {
  --live-profile-accent: #b7b7b7;
  --live-profile-surface: #292929;
}

html[data-theme="dark"] .app-shell .profile-main-row {
  border: 0 !important;
  background: var(--live-profile-surface) !important;
  box-shadow: none !important;
  color: #ededed !important;
}

html[data-theme="dark"] .app-shell :is(.profile-main-row, .profile-stat-row).profile-tier-apex {
  --live-profile-accent: #c7a15d;
  --live-profile-surface: #302a21;
}

html[data-theme="dark"] .app-shell :is(.profile-main-row, .profile-stat-row).profile-tier-steady {
  --live-profile-accent: #59d99b;
  --live-profile-surface: #202c25;
}

html[data-theme="dark"] .app-shell :is(.profile-main-row, .profile-stat-row).profile-tier-normal {
  --live-profile-accent: #75baff;
  --live-profile-surface: #222b35;
}

html[data-theme="dark"] .app-shell :is(.profile-main-row, .profile-stat-row).profile-tier-small-pit {
  --live-profile-accent: #f0b45e;
  --live-profile-surface: #322b21;
}

html[data-theme="dark"] .app-shell :is(.profile-main-row, .profile-stat-row).profile-tier-big-pit {
  --live-profile-accent: #ff7777;
  --live-profile-surface: #332325;
}

html[data-theme="dark"] .app-shell .profile-main-row b {
  color: var(--live-profile-accent) !important;
}

html[data-theme="dark"] .app-shell .profile-stat-row {
  border: 0 !important;
  border-radius: 0 !important;
  background: transparent !important;
  box-shadow: none !important;
  color: #b6b6b6 !important;
  padding: 6px 3px 1px !important;
}

html[data-theme="dark"] .app-shell .profile-stat-row span {
  color: #b6b6b6 !important;
}

/* Historical match rows use quiet result tints, neutral outlines, and a slim status rail. */
html[data-theme="dark"] .app-shell .recent-game {
  border-width: 1px !important;
}

html[data-theme="dark"] .app-shell .recent-game.win {
  border-color: #4a4a4a !important;
  background: #202c25 !important;
}

html[data-theme="dark"] .app-shell .recent-game.loss {
  border-color: #4a4a4a !important;
  background: #332325 !important;
}

html[data-theme="dark"] .app-shell .recent-game.win:hover {
  border-color: #5a5a5a !important;
  background: #26352c !important;
  border-left-color: #599c72 !important;
}

html[data-theme="dark"] .app-shell .recent-game.loss:hover {
  border-color: #5a5a5a !important;
  background: #3b292c !important;
  border-left-color: #ca4b4b !important;
}

html[data-theme="dark"] .app-shell .recent-game {
  border-left-width: 3px !important;
}

html[data-theme="dark"] .app-shell .recent-game.win {
  border-left-color: #599c72 !important;
}

html[data-theme="dark"] .app-shell .recent-game.loss {
  border-left-color: #ca4b4b !important;
}

/* Apply the same quiet result treatment to the current player's match history. */
html[data-theme="dark"] .app-shell .record-row.interactive {
  border-width: 1px !important;
  border-left-width: 5px !important;
  box-shadow: none !important;
}

html[data-theme="dark"] .app-shell .record-row.interactive.win {
  border-color: #4a4a4a !important;
  border-left-color: #599c72 !important;
  background: #202c25 !important;
}

html[data-theme="dark"] .app-shell .record-row.interactive.lose {
  border-color: #4a4a4a !important;
  border-left-color: #ca4b4b !important;
  background: #332325 !important;
}

html[data-theme="dark"] .app-shell .record-row.interactive.win:hover {
  border-color: #5a5a5a !important;
  border-left-color: #599c72 !important;
  background: #26352c !important;
  filter: none !important;
}

html[data-theme="dark"] .app-shell .record-row.interactive.lose:hover {
  border-color: #5a5a5a !important;
  border-left-color: #ca4b4b !important;
  background: #3b292c !important;
  filter: none !important;
}

/* Score cells use a bright border matching their rating tier. */
html[data-theme="dark"] .app-shell .score-cell.score-excellent {
  border: 1px solid #c7a15d !important;
}

html[data-theme="dark"] .app-shell .score-cell.score-good {
  border: 1px solid #59d99b !important;
}

html[data-theme="dark"] .app-shell .score-cell.score-average {
  border: 1px solid #75baff !important;
}

html[data-theme="dark"] .app-shell .score-cell.score-poor {
  border: 1px solid #ff7777 !important;
}

/* The match detail dialog is teleported outside .app-shell, so it needs its own dark rules. */
html[data-theme="dark"] body .match-detail-tab,
html[data-theme="dark"] body .match-detail-tab.active {
  color: #f4f4f4 !important;
  border: 1px solid #4a4a4a !important;
  background: #303030 !important;
  box-shadow: none !important;
}

html[data-theme="dark"] body .match-detail-modal {
  border-color: #3b3b3b !important;
  background: #1f1f1f !important;
  box-shadow: 0 24px 80px rgb(0 0 0 / 42%) !important;
}

html[data-theme="dark"] body .match-detail-modal .match-detail-toolbar {
  border-bottom-color: #3b3b3b !important;
  background: #252525 !important;
}

html[data-theme="dark"] body .match-detail-modal .match-detail-body {
  background: #1f1f1f !important;
}

html[data-theme="dark"] body .match-detail-modal .match-detail-team {
  border-color: #3b3b3b !important;
  background: #252525 !important;
}

html[data-theme="dark"] body .match-detail-modal .match-detail-team-header {
  border: 1px solid #4a4a4a !important;
}

html[data-theme="dark"] body .match-detail-modal .match-detail-team-header.win {
  color: #a8eabc !important;
  background: #202c25 !important;
}

html[data-theme="dark"] body .match-detail-modal .match-detail-team-header.lose {
  color: #ff9696 !important;
  background: #332325 !important;
}

html[data-theme="dark"] body .match-detail-modal .record-row.detail-row {
  border-width: 1px !important;
  border-left-width: 5px !important;
  box-shadow: none !important;
}

html[data-theme="dark"] body .match-detail-modal .record-row.detail-row.win {
  border-color: #4a4a4a !important;
  border-left-color: #599c72 !important;
  background: #202c25 !important;
}

html[data-theme="dark"] body .match-detail-modal .record-row.detail-row.lose {
  border-color: #4a4a4a !important;
  border-left-color: #ca4b4b !important;
  background: #332325 !important;
}

html[data-theme="dark"] body .match-detail-modal .record-row.detail-row .queue-name {
  color: #e1e1e1 !important;
}

html[data-theme="dark"] body .match-detail-modal .record-row.detail-row.win .queue-name {
  color: #a8eabc !important;
}

html[data-theme="dark"] body .match-detail-modal .record-row.detail-row.lose .queue-name {
  color: #ff9696 !important;
}

html[data-theme="dark"] body .match-detail-modal .score-cell.score-excellent {
  border: 1px solid #c7a15d !important;
}

html[data-theme="dark"] body .match-detail-modal .score-cell.score-good {
  border: 1px solid #59d99b !important;
}

html[data-theme="dark"] body .match-detail-modal .score-cell.score-average {
  border: 1px solid #75baff !important;
}

html[data-theme="dark"] body .match-detail-modal .score-cell.score-poor {
  border: 1px solid #ff7777 !important;
}

/* The standalone detail tab has its own component tree, separate from the modal detail view. */
html[data-theme="dark"] .app-shell .overview-panel {
  border-color: #3b3b3b !important;
  background: #1f1f1f !important;
}

html[data-theme="dark"] .app-shell .overview-toolbar {
  border-bottom-color: #3b3b3b !important;
  background: #252525 !important;
}

html[data-theme="dark"] .app-shell .overview-body {
  background: #1f1f1f !important;
}

html[data-theme="dark"] .app-shell .team-block {
  border-color: #3b3b3b !important;
  background: #252525 !important;
}

html[data-theme="dark"] .app-shell .team-header.win {
  border: 1px solid #4a4a4a !important;
  color: #a8eabc !important;
  background: #202c25 !important;
}

html[data-theme="dark"] .app-shell .team-header.lose {
  border: 1px solid #4a4a4a !important;
  color: #ff9696 !important;
  background: #332325 !important;
}

html[data-theme="dark"] .app-shell .detail-row {
  border-width: 1px !important;
  border-left-width: 5px !important;
  box-shadow: none !important;
}

html[data-theme="dark"] .app-shell .detail-row.win {
  border-color: #4a4a4a !important;
  border-left-color: #599c72 !important;
  background: #202c25 !important;
}

html[data-theme="dark"] .app-shell .detail-row.lose {
  border-color: #4a4a4a !important;
  border-left-color: #ca4b4b !important;
  background: #332325 !important;
}

html[data-theme="dark"] .app-shell .detail-row.win .detail-player-name {
  color: #a8eabc !important;
}

html[data-theme="dark"] .app-shell .detail-row.lose .detail-player-name {
  color: #ff9696 !important;
}

html[data-theme="dark"] .app-shell .detail-rank-tag {
  border-color: #a98c43 !important;
  color: #f5dda0 !important;
  background: #4b3b1a !important;
}

html[data-theme="dark"] .app-shell .detail-rank-tag.detail-rank-unranked {
  border-color: #666666 !important;
  color: #c3c3c3 !important;
  background: #3b3b3b !important;
}

html[data-theme="dark"] .app-shell .detail-level-tag {
  border-color: #5b9b6c !important;
  color: #b8e9c3 !important;
  background: #1d3a27 !important;
}

html[data-theme="dark"] .app-shell .detail-rank-loading {
  color: #aaaaaa !important;
}

html[data-theme="dark"] .app-shell .detail-row.win:hover {
  border-color: #5a5a5a !important;
  border-left-color: #599c72 !important;
  background: #26352c !important;
  filter: none !important;
}

html[data-theme="dark"] .app-shell .detail-row.lose:hover {
  border-color: #5a5a5a !important;
  border-left-color: #ca4b4b !important;
  background: #3b292c !important;
  filter: none !important;
}

/* Stack a team's rows into one continuous result surface. */
html[data-theme="dark"] .app-shell .overview-panel .overview-body:not(.chart-mode) .detail-list {
  gap: 0 !important;
  overflow: hidden;
  border-radius: 6px;
}

html[data-theme="dark"] .app-shell .overview-panel .overview-body:not(.chart-mode) .detail-row {
  border: 0 !important;
  border-left: 5px solid transparent !important;
  border-radius: 0 !important;
}

html[data-theme="dark"] .app-shell .overview-panel .overview-body:not(.chart-mode) .detail-row.win {
  border-left-color: #599c72 !important;
}

html[data-theme="dark"] .app-shell .overview-panel .overview-body:not(.chart-mode) .detail-row.lose {
  border-left-color: #ca4b4b !important;
}

html[data-theme="dark"] .app-shell .overview-panel .overview-body:not(.chart-mode) .detail-row:first-child {
  border-radius: 6px 6px 0 0 !important;
}

html[data-theme="dark"] .app-shell .overview-panel .overview-body:not(.chart-mode) .detail-row:last-child {
  border-radius: 0 0 6px 6px !important;
}
`

const DARK_THEME_FIXES = {
  invert: [],
  css: "",
  ignoreInlineStyle: [],
  ignoreImageAnalysis: [
    ".champion-avatar img",
    ".asset-icon img",
    ".augment-icon img",
    ".hextech-icon img",
    ".rank-icon",
    ".rank-icon img",
    ".rank-card-icon",
    "img[src^='asset:']",
    "img[src^='data:']",
  ],
  disableStyleSheetsProxy: false,
  ignoreCSSUrl: [],
}

function syncDarkThemeCss() {
  let style = document.getElementById(THEME_STYLE_ID)
  if (!style) {
    style = document.createElement("style")
    style.id = THEME_STYLE_ID
    style.classList.add("darkreader")
    document.head.appendChild(style)
  }
  style.textContent = DARK_THEME_CSS
}

export function applyTheme(darkModeEnabled: boolean) {
  document.documentElement.dataset.theme = darkModeEnabled ? "dark" : "light"
  document.documentElement.style.colorScheme = darkModeEnabled ? "dark" : "light"

  if (darkModeEnabled) {
    enable(DARK_THEME, DARK_THEME_FIXES)
    syncDarkThemeCss()
  } else {
    disable()
    document.getElementById(THEME_STYLE_ID)?.remove()
  }
}

if (import.meta.hot) {
  import.meta.hot.accept(() => {
    window.location.reload()
  })
}
