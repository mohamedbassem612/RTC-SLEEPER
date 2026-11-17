package main

import "C"
import "tailscale.com/types/logger"

// Keep interactive enrollment URLs in the UI instead of process/system logs.
//
//export RtcQuiet
func RtcQuiet(sd C.int) {
	if server := getServer(sd); server != nil {
		server.s.UserLogf = logger.Discard
		server.s.Logf = logger.Discard
	}
}
