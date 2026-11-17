package main

import "C"
import "os"
import "path/filepath"
import "runtime"
import "tailscale.com/types/logger"

// Go maintains its own environment snapshot in a shared-library process.
// Initialize its paths explicitly before tsnet creates logging/sockstat state.
//
//export RtcStateDir
func RtcStateDir(directory *C.char) C.int {
	dir := C.GoString(directory)
	if !filepath.IsAbs(dir) {
		return -1
	}
	if err := os.MkdirAll(dir, 0700); err != nil {
		return -1
	}
	if err := os.Setenv("TS_LOGS_DIR", dir); err != nil {
		return -1
	}
	if runtime.GOOS == "android" {
		for _, key := range []string{"HOME", "TMPDIR", "XDG_CACHE_HOME"} {
			if err := os.Setenv(key, dir); err != nil {
				return -1
			}
		}
	}
	return 0
}

// Keep interactive enrollment URLs in the UI instead of process/system logs.
//
//export RtcQuiet
func RtcQuiet(sd C.int) {
	if server := getServer(sd); server != nil {
		server.s.UserLogf = logger.Discard
		server.s.Logf = logger.Discard
	}
}
