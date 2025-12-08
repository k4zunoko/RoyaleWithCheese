```
cargo test  dda -- --ignored --nocapture --test-threads=1
```

```
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.10s
     Running unittests src\main.rs (target\debug\deps\RoyaleWithCheese-6e6fbc0025155bcd.exe)

running 4 tests
test infrastructure::capture::dda::tests::test_dda_capture_multiple_frames ... Capture error: DeviceNotAvailable
Capture statistics (1 second):
  Frames captured: 15
  Timeouts: 0
  Effective FPS: 15
ok
test infrastructure::capture::dda::tests::test_dda_capture_single_frame ...
thread 'infrastructure::capture::dda::tests::test_dda_capture_single_frame' (138420) panicked at src\infrastructure\capture\dda.rs:245:14:
Frame capture failed: DeviceNotAvailable
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
FAILED
test infrastructure::capture::dda::tests::test_dda_initialization ... Device Info:
  Resolution: 1920x1080
  Refresh Rate: 144Hz
  Name: Display 0 on Adapter 0
ok
test infrastructure::capture::dda::tests::test_dda_reinitialize ... Reinitialization failed (expected due to DDA API limitation): Some(Capture("Failed to reinitialize DDA: BadParam(\"failed to create duplicate output. Error { code: HRESULT(0x80070057), message: \\\"パラメーターが間違っています。\\\" }\")"))
ok

failures:

failures:
    infrastructure::capture::dda::tests::test_dda_capture_single_frame

test result: FAILED. 3 passed; 1 failed; 0 ignored; 0 measured; 32 filtered out; finished in 0.83s

error: test failed, to rerun pass `--bin RoyaleWithCheese`

```

```
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.10s
     Running unittests src\main.rs (target\debug\deps\RoyaleWithCheese-6e6fbc0025155bcd.exe)

running 4 tests
test infrastructure::capture::dda::tests::test_dda_capture_multiple_frames ... Capture statistics (1 second):
  Frames captured: 144
  Timeouts: 0
  Effective FPS: 144
ok
test infrastructure::capture::dda::tests::test_dda_capture_single_frame ... Captured frame:
  Size: 1920x1080
  Data length: 8294400 bytes
  Expected: 8294400 bytes
ok
test infrastructure::capture::dda::tests::test_dda_initialization ... Device Info:
  Resolution: 1920x1080
  Refresh Rate: 144Hz
  Name: Display 0 on Adapter 0
ok
test infrastructure::capture::dda::tests::test_dda_reinitialize ... Reinitialization failed (expected due to DDA API limitation): Some(Capture("Failed to reinitialize DDA: BadParam(\"failed to create duplicate output. Error { code: HRESULT(0x80070057), message: \\\"パラメーターが間違っています。\\\" }\")"))
ok

test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 32 filtered out; finished in 1.48s
```