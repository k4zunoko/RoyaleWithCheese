```
cargo test  dda -- --ignored --nocapture --test-threads=1
```

```
cargo test test_exclusive_fullscreen_recovery -- --ignored --nocapture --test-threads=1
```


```
     Running unittests src\main.rs (target\debug\deps\RoyaleWithCheese-6e6fbc0025155bcd.exe)

running 1 test
test application::recovery::integration_tests::test_exclusive_fullscreen_recovery ... 
=== Exclusive Fullscreen Recovery Test ===
Prerequisites:
  - Exclusive fullscreen app running on primary monitor
  - 144Hz monitor environment

DDA initialized:
  Resolution: 1920x1080
  Refresh Rate: 144Hz

Starting capture loop (3 seconds)...


=== Test Results ===
Duration: 3.00s

Frame Statistics:
  Frames captured: 388
  Timeouts: 0
  DeviceNotAvailable errors: 0
  ReInitializationRequired errors: 0
  Effective FPS: 129.25

Recovery Statistics:
  Total reinitializations: 0
  Successful reinitializations: 0
  Current backoff: 50ms
  Cumulative failure exceeded: false

Validation (Desktop Environment):

thread 'application::recovery::integration_tests::test_exclusive_fullscreen_recovery' (78520) panicked at src\application\recovery.rs:465:13:
Expected FPS >= 129.60, got 129.25
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
FAILED

failures:

failures:
    application::recovery::integration_tests::test_exclusive_fullscreen_recovery

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 36 filtered out; finished in 3.16s

error: test failed, to rerun pass `--bin RoyaleWithCheese`
```