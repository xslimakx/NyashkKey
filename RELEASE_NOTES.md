## NyashkKey 1.0.1

- Захист від NaN / inf від хоста: один «битий» семпл більше не ламає плагін назавжди (раніше фільтри залишались у NaN і звук пропадав до перезапуску). Раніше один NaN робив гістограму NaN і тональність переставала визначатись.

## NyashkKey 1.0.0

First release.

- Finds the **key** of whatever plays through it (major / minor in all 12 keys) with a confidence bar, the relative key, the Camelot code and the two next best candidates.
- Finds the **tuning**: how many cents the audio is off 440 Hz, e.g. after speeding a sample up, and the matching **Ref A4** (e.g. +39 c = 449.9 Hz).
- **NOTES** view: which notes sound over the whole analysis and right now; scale notes and the tonic are marked.
- **FOR NYASHKTUNE**: Key, Scale (Major / Natural Minor / Harmonic Minor) and Ref A4 ready to type into NyashkTune.
- HOLD, RESET, note spelling AUTO / ♯ / ♭. The audio passes through untouched.
- Tested on chord progressions in all 24 keys at random tunings (tuning error under 1 cent) and on real recordings, sped up, slowed down and transposed.
