// Cue times (seconds) shared by ad.html (visuals) and audio.py (soundtrack), so they can't drift apart.
// audio.py reads the {...} below as JSON: keep it plain JSON (double quotes, no comments inside).
window.CUES = {
  "cuts": [2.0, 5.5, 9.0, 12.0],
  "trafficRows": [2.35, 0.08],
  "tooltip": 4.0,
  "firewallRows": [5.7, 0.07],
  "click": 7.0,
  "toast": 7.15,
  "events": [9.3, 0.42],
  "dropout": 11.86,
  "ruleLight": 13.3
};
