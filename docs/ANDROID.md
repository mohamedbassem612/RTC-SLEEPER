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
5. On the desktop choose **New pairing code**. On Android choose **Scan QR Code**, or enter/paste the **24-word pairing phrase**, then select **Pair Laptop**.

Tailscale enrollment and application pairing are separate. The QR/phrase authorizes this phone to control this laptop; it does not contain a Tailscale auth key or bypass network enrollment. Camera permission is requested only when scanning. Phrase entry works without a camera.

The desktop code expires after 10 minutes and is consumed after one successful pairing. Creating another code invalidates the previous one. Codes encode a version, Tailscale IPv4 address, port, 128-bit random secret, expiry and checksum. The word encoding uses the BIP39 English vocabulary but is **not a cryptocurrency wallet mnemonic**. A successful pairing issues a separate 256-bit device token; Android encrypts it using Android Keystore AES-GCM, and app backup is disabled.

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

Release builds require `RTC_KEYSTORE` (path) and `RTC_KEYSTORE_PASSWORD`; alias is `rtc-sleeper`. Signing keys are not committed. `RTC_ANDROID_ABIS` can restrict a local build to `x86_64` or `arm64-v8a`. `RTC_GRADLE` can select a specific Gradle executable. The official libtailscale source is pinned to `dacde75057ff350e146505829beff446e6823966`; its BSD license is included in generated bundles. Android builds opt into Tailscale's official `androidbin` support for Android network-interface/DNS/certificate restrictions.

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

## GitHub releases

`Android Rust build and release` runs Rust and mock-integration tests, builds arm64/x86-64 native code, embeds Tailscale, signs a universal APK and uploads artifacts. An `android-v*` tag publishes an Android-specific GitHub Release plus the Linux companion. Release signing uses repository secrets `RTC_ANDROID_KEYSTORE` and `RTC_ANDROID_KEYSTORE_PASSWORD`. Fork pull requests build a debug APK without release secrets.

The mobile app is a separate component; the original Python release remains on `python-version`, and the C++ desktop code remains on `main`.
