package main

import (
	"context"
	"errors"
	"net"
	"testing"
	"time"
)

func TestRtcDeadlineCancelsUnavailableDial(t *testing.T) {
	started := time.Now()
	_, err := rtcDialBounded(func(ctx context.Context, _, _ string) (net.Conn, error) { <-ctx.Done(); return nil, ctx.Err() }, "tcp", "offline", 40*time.Millisecond)
	if !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("expected deadline, got %v", err)
	}
	if time.Since(started) > time.Second {
		t.Fatal("connection did not release promptly")
	}
}

func TestRtcDeadlineAlsoBoundsSilentPeer(t *testing.T) {
	peer, server := net.Pipe()
	defer server.Close()
	connection, err := rtcDialBounded(func(context.Context, string, string) (net.Conn, error) { return peer, nil }, "tcp", "silent", 40*time.Millisecond)
	if err != nil {
		t.Fatal(err)
	}
	defer connection.Close()
	started := time.Now()
	_, err = connection.Read(make([]byte, 1))
	if e, ok := err.(net.Error); !ok || !e.Timeout() {
		t.Fatalf("expected read timeout, got %v", err)
	}
	if time.Since(started) > time.Second {
		t.Fatal("silent peer kept request busy")
	}
}
