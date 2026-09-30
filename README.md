# ecgdisp

Display 10-second PTB-XL ECG records in an egui window.

```sh
cargo run --release -- [RECORD] [-l LEADS] [-d DIR]
cargo run --release -- 42 -l I,II,V1   # record 00042_hr, three leads
```

Hover over the traces to read each lead's value (mV) at the cursor time.
Use Left/Right (or PgUp/PgDn) to step to the previous/next record in the same folder.
