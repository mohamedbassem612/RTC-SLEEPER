# RTC Sleeper Android — Rust

The Android UI, pairing protocol, validation and remote client are Rust. A small Java NativeActivity adapter provides Android Keystore encryption, browser sign-in and the embedded QR camera scanner. The official `libtailscale` Go library is embedded in the APK and Linux companion; a separate Tailscale application is not required.

## Setup

1. Install the C++ desktop application from current `main` (see the main README).
2. Install the remote companion as the desktop user, without sudo:

   ```bash
   # From source, after building both prerequisites:
   bash scripts/install-remote.sh
   # From the companion release archive:
   bash scripts/install-remote.sh ./rtc-sleeper-remote ./libtailscale.so
   ```

3. In the desktop app select **Settings → Remote Control → Sign in to private network**. Authenticate in your browser with your Tailscale account, then refresh the connection.
4. Install the signed Android APK. Open RTC Sleeper, sign into the **same tailnet**, then return to the app.
5. On the desktop open **Settings → Remote Control**. The QR and **24-word pairing phrase** appear automatically once connected. On Android choose **Scan QR Code**, or enter/paste the phrase, then select **Pair Laptop**. Select the new phone in the desktop list and click **Allow selected**. Until approved, it cannot read status or send commands. Use the same QR/phrase to add another phone; each needs separate approval.

Tailscale enrollment and application pairing are separate. The QR/phrase authorizes this phone to control this laptop; it does not contain a Tailscale auth key or bypass network enrollment. Camera permission is requested only when scanning. Phrase entry works without a camera.

The desktop code is reusable, has no time limit and survives service restarts. **Show QR / pairing phrase** displays the existing code; **Regenerate pairing code** replaces it without disconnecting existing phones. Codes encode a version, Tailscale IPv4 address, port, 128-bit random secret, a non-expiring marker and checksum. The word encoding uses the BIP39 English vocabulary but is **not a cryptocurrency wallet mnemonic**. Each successful pairing issues a separate 256-bit device token; Android encrypts it using Android Keystore AES-GCM, and app backup is disabled. Existing Android 0.1.2 clients accept the new codes.

The desktop is the access authority. **Settings → Remote Control** lists paired devices with separate IDs and allowed/blocked states. Select a device and use **Allow selected** or **Block selected**; a blocked phone's existing token immediately stops working. **Remote access: OFF** rejects all remote requests, including new pairings, without logging the laptop out of Tailscale. Permissions and the global switch survive service restarts. These administration operations are available only through the laptop's private local control socket, not the mobile protocol.

**Revoke all paired phones** invalidates all device tokens on the laptop. Forgetting a laptop on the phone removes its local credential; revoke it on the laptop as well to invalidate server-side access.

## Controls

- Status, live RTC alarm and time until scheduled sleep.
- Sleep and wake times, Sleep Now, weekly repeat.
- Apply, Pause/Resume, Remove and activity logs.
- Mutating commands require an explicit confirmation. Sleep Now warns that it will suspend the laptop in about five seconds.
- There is no offline command queue and no automatic retry of a mutating action. Requests expire after 30 seconds, and duplicate request IDs are rejected during the replay window. Status polling resumes when the phone is online again.
- Remote commands use the existing validated C++ CLI/helper. The remote process runs as the desktop user, not root. An actor label such as `Android: RTC Sleeper Android` appears in engine logs alongside the OS UID.
- The phone cannot contact a laptop while it is suspended or offline. A previously armed RTC alarm still controls scheduled wake. This does not implement remote Wake-on-LAN or shutdown.

The production listener is inside the embedded tailnet, not a public/LAN TCP port. Tailnet access rules still apply. The Tailscale network supplies end-to-end encrypted transport; no public HTTP endpoint or Funnel is enabled. Tailscale coordination/relay services remain a dependency. Neither Internet availability nor firmware wake behavior is guaranteed by the application.

## Local build

Prerequisites: Rust 1.100.0, Go 1.27.4, JDK 21, Gradle 9.4.0, Android platform/build tools 36, and NDK 28.2.13676358. Set `ANDROID_HOME` to the Android SDK and add Rust, Go and Gradle to PATH.

```bash
rustup target add aarch64-linux-android x86_64-linux-android
sdkmanager 'platforms;android-36' 'build-tools;36.0.0' 'ndk;28.2.13676358'
bash scripts/build-android.sh debug
# APK: android/app/build/outputs/apk/debug/app-debug.apk

# Build the Linux companion:
bash scripts/build-tailnet.sh linux dist/remote
cargo build --manifest-path remote/Cargo.toml --release -p rtc-sleeper-remote
```

Release builds require `RTC_KEYSTORE` (path) and `RTC_KEYSTORE_PASSWORD`; alias is `rtc-sleeper`. Signing keys are not committed. `RTC_ANDROID_ABIS` can restrict a local build to `x86_64` or `arm64-v8a`. `RTC_GRADLE` can select a specific Gradle executable. The official libtailscale source is pinned to `dacde75057ff350e146505829beff446e6823966`; its BSD license is included in generated bundles. Android's `ConnectivityManager` supplies interface names, addresses, MTUs and default gateway through Rust to a registered `netmon` getter **before** tsnet startup. The snapshot refreshes during network/status polling and before remote requests; changes notify netmon. This avoids Android 11+ blocking Go's NETLINK_ROUTE access. Importing `androidbin` alone is insufficient: its interface fallback explicitly excludes Android CGO builds such as our shared library.

## Tests — never suspend the development machine

```bash
cargo test --locked --manifest-path remote/Cargo.toml --workspace
cargo build --locked --manifest-path remote/Cargo.toml -p rtc-sleeper-remote
python3 -m unittest discover -s tests -p test_remote.py -v
ctest --test-dir build --output-on-failure
```

The integration fixture launches `rtc-sleeper-remote serve --mock` bound only to `127.0.0.1:48188`, with isolated temporary state. Its backend simulates all actions and does not invoke the C++ power helper. Debug Android builds may access this fixture through the emulator-only address `10.0.2.2`; release builds reject that route and ignore debug pairing intent extras. Test fixtures are never enabled by the installed production service.

To inspect the Android UI in a configured emulator:

```bash
adb install -r android/app/build/outputs/apk/debug/app-debug.apk
adb shell am start -n com.mohamedbassem.rtcsleeper/.MainActivity
```

Real off-network encrypted connectivity requires enrolling both nodes with a live Tailscale account and testing on separate networks. Local unit/emulator tests cannot replace that check.

Android sandbox regression (run on an online API 30+ emulator):

```bash
RTC_ANDROID_ABIS=x86_64 bash scripts/build-android.sh debug
android/gradlew -p android :app:assembleDebugAndroidTest
adb install -r android/app/build/outputs/apk/debug/app-debug.apk
adb install -r android/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk
adb shell am instrument -w com.mohamedbassem.rtcsleeper.test/androidx.test.runner.AndroidJUnitRunner
```

`NetworkStartupTest` launches the real embedded node in the app UID, checks the public Android network snapshot, rejects any startup error and waits for `NeedsLogin` or `Running`. It does not enroll an account or send any scheduler request. CI runs this test on Android 11 before publishing APKs; only the release APK is attached to tagged releases.

Starting with 0.1.6, the same test also exercises a real tsnet dial with a short deadline while unenrolled, then a debug-only loopback laptop which replies `idle`, stops replying, and recovers. It checks that the UI stops being busy after a timeout, keeps the last confirmed status and recovers on a later read-only poll. The fixture never sends power commands. Production RPCs have one 12-second absolute deadline covering connect and response; there is no automatic retry of mutating actions. Go tests additionally verify cancellation of an unavailable dial and timeout of a silent peer. Reopening the app restores tsnet credentials instead of forcing interactive login; if login becomes necessary, the sign-in option remains visible after application pairing.

The complete 0.1.6 Android startup/deadline/loss/recovery regression passed locally on API 30 (`OK (1 test)`, 17.734 seconds). Android's `EAGAIN` read timeout may be reported as `Try again (os error 11)`; the UI translates it into an actionable connection timeout message rather than exposing the raw OS wording.

For 0.1.4 this regression passed locally on the Android 11/API 30 AOSP x86_64 emulator (`OK (1 test)`). Both ARM64 and x86_64 signed release binaries also built and the APK signature verified. The earlier API 29 smoke test did not cover Android 11's NETLINK_ROUTE restriction; the new instrumentation test explicitly requires API 30+.

### Local validation for Android 0.1.2

- Rust pairing/validation/authorization tests and the isolated loopback integration test passed; existing C++ engine/interface tests passed.
- Built a signed universal release APK for arm64-v8a and x86_64, passed release lint and verified its APK signature.
- On the Android 10/API 29 x86_64 emulator: launched the embedded Tailscale node, paired with the mock laptop, applied a simulated schedule, paused/removed it and displayed logs. Pairing survived an app restart using encrypted Android Keystore storage.
- Opened the native phrase-entry dialog, granted camera permission and displayed the live emulator camera scanner. QR payload decoding is covered by the Rust test; scanning a physical laptop screen with a phone remains a device check.
- No real suspend, shutdown or RTC-alarm modification was used for these tests. Live enrollment and separate-network connectivity still require the account setup above.

## GitHub releases

`Android Rust build and release` runs Rust and mock-integration tests, builds arm64/x86-64 native code, embeds Tailscale, signs a universal APK and uploads artifacts. An `android-v*` tag publishes an Android-specific GitHub Release plus the Linux companion. Release signing uses repository secrets `RTC_ANDROID_KEYSTORE` and `RTC_ANDROID_KEYSTORE_PASSWORD`. Fork pull requests build a debug APK without release secrets.

The mobile app is a separate component; the original Python release remains on `python-version`, and the C++ desktop code remains on `main`.
