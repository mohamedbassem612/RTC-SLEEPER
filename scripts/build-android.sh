#!/bin/bash
set -euo pipefail
repo_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
build_type="${1:-debug}"
[[ "$build_type" == debug || "$build_type" == release ]] || exit 1
sdk_dir="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}"
ndk_dir="${ANDROID_NDK_HOME:-$sdk_dir/ndk/28.2.13676358}"
abi_list="${RTC_ANDROID_ABIS:-arm64-v8a x86_64}"
for abi in $abi_list; do
    case "$abi" in arm64-v8a) triple=aarch64-linux-android ;; x86_64) triple=x86_64-linux-android ;; *) exit 1 ;; esac
    jni_dir="$repo_dir/android/app/src/main/jniLibs/$abi"
    mkdir -p "$jni_dir"
    if [[ ! -f "$jni_dir/libtailscale.so" ]]; then bash "$repo_dir/scripts/build-tailnet.sh" "$abi" "$jni_dir"; fi
    linker="$ndk_dir/toolchains/llvm/prebuilt/linux-x86_64/bin/${triple}26-clang"
    env_name="CARGO_TARGET_$(tr '[:lower:]-' '[:upper:]_' <<< "$triple")_LINKER"
    cargo_flags=(); [[ "$build_type" == release ]] && cargo_flags+=(--release)
    (cd "$repo_dir/remote" && env "$env_name=$linker" RUSTFLAGS='-C link-arg=-Wl,-z,max-page-size=16384' cargo build --locked --jobs 2 -p rtc-sleeper-android --target "$triple" "${cargo_flags[@]}")
    cp "$repo_dir/remote/target/$triple/$build_type/librtc_sleeper_android.so" "$jni_dir/"
done
gradle_command="${RTC_GRADLE:-$repo_dir/android/gradlew}"
(cd "$repo_dir/android" && "$gradle_command" --no-daemon "assemble${build_type^}")
