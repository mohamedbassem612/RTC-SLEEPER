#!/bin/bash
set -euo pipefail
repo_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
vendor_dir="${RTC_TAILSCALE_SOURCE:-$repo_dir/.tools/libtailscale}"
revision=dacde75057ff350e146505829beff446e6823966
if [[ ! -d "$vendor_dir/.git" ]]; then
    git clone https://github.com/tailscale/libtailscale.git "$vendor_dir"
fi
[[ "$(git -C "$vendor_dir" rev-parse HEAD)" == "$revision" ]] || git -C "$vendor_dir" checkout "$revision"
cp "$repo_dir/remote/tailnet/rtc_android.go" "$vendor_dir/rtc_android.go"
cp "$repo_dir/remote/tailnet/rtc_quiet.go" "$vendor_dir/rtc_quiet.go"
cp "$repo_dir/remote/tailnet/rtc_deadline.go" "$vendor_dir/rtc_deadline.go"
cp "$repo_dir/remote/tailnet/rtc_transport.go" "$vendor_dir/rtc_transport.go"
cp "$repo_dir/remote/tailnet/rtc_deadline_test.go" "$vendor_dir/rtc_deadline_test.go"
target="${1:-linux}"
output_dir="${2:-$repo_dir/dist/remote}"
mkdir -p "$output_dir"
output_dir="$(cd "$output_dir" && pwd)"
if [[ "$target" == linux ]]; then
    (cd "$vendor_dir" && GOMAXPROCS=2 go test -p 2 rtc_transport.go rtc_deadline_test.go)
    (cd "$vendor_dir" && GOMAXPROCS=2 CGO_ENABLED=1 go build -p 2 -buildmode=c-shared -ldflags='-s -w' -o "$output_dir/libtailscale.so" .)
else
    sdk_dir="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}"
    ndk_dir="${ANDROID_NDK_HOME:-$sdk_dir/ndk/28.2.13676358}"
    case "$target" in
        arm64-v8a) compiler=aarch64-linux-android26-clang; arch=arm64 ;;
        x86_64) compiler=x86_64-linux-android26-clang; arch=amd64 ;;
        *) echo 'Use linux | arm64-v8a | x86_64' >&2; exit 1 ;;
    esac
    (cd "$vendor_dir" && GOMAXPROCS=2 GOOS=android GOARCH="$arch" CGO_ENABLED=1 CC="$ndk_dir/toolchains/llvm/prebuilt/linux-x86_64/bin/$compiler" CGO_LDFLAGS='-Wl,-z,max-page-size=16384' go build -p 2 -buildmode=c-shared -ldflags='-s -w' -o "$output_dir/libtailscale.so" .)
fi
cp "$vendor_dir/LICENSE" "$output_dir/TAILSCALE-LICENSE"
