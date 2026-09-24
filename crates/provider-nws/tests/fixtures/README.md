# NWS fixtures

From `https://api.weather.gov/alerts/active` on 2026-09-23 (about 7 PM CT).
NWS data is public domain.

| File | Source |
|---|---|
| `active_severe.json` | **Real.** `?status=actual&message_type=alert&severity=Severe,Extreme`, first 4 alerts (flood and flash flood warnings in WV, AZ, NM), properties trimmed to the fields we read. |
| `point_none.json` | **Real.** `?point=39.0997,-94.5786` (Kansas City): no alerts. |
| `point_out_of_bounds.json` | **Real.** `?point=59.91,10.75` (Oslo): the 400 "out of bounds" problem. |
| `synthetic_kc.json` | **Synthetic.** Built from a real alert's shape: a tornado warning, a severe thunderstorm watch, a wind advisory, a special weather statement, plus a cancellation and a test message that must be dropped. |
