package main

// #include <errno.h>
import "C"
import (
	"time"
)

//export RtcDialDeadline
func RtcDialDeadline(sd C.int, address *C.char, timeoutMillis C.int, out *C.int) C.int {
	s := getServer(sd)
	if s == nil {
		return C.EBADF
	}
	if out == nil || timeoutMillis <= 0 || timeoutMillis > 30000 {
		return C.EINVAL
	}
	*out = -1
	connection, err := rtcDialBounded(s.s.Dial, "tcp", C.GoString(address), time.Duration(timeoutMillis)*time.Millisecond)
	if err != nil {
		return s.recErr(err)
	}
	s.started = true
	if err := newConn(s, connection, out); err != nil {
		connection.Close()
		return s.recErr(err)
	}
	return C.int(0)
}
