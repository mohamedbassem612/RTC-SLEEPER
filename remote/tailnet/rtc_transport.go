package main

import (
	"context"
	"net"
	"time"
)

// One absolute deadline covers both connection establishment and the RPC.
// The upstream C dial uses context.Background and can otherwise wait forever.
func rtcDialBounded(dial func(context.Context, string, string) (net.Conn, error), network, address string, timeout time.Duration) (net.Conn, error) {
	deadline := time.Now().Add(timeout)
	ctx, cancel := context.WithDeadline(context.Background(), deadline)
	defer cancel()
	connection, err := dial(ctx, network, address)
	if err != nil {
		return nil, err
	}
	if err := connection.SetDeadline(deadline); err != nil {
		connection.Close()
		return nil, err
	}
	return connection, nil
}
