//go:build android

package main

// Android c-shared builds must provide a Java-backed interface getter.
// androidbin's registration deliberately excludes android && cgo.

import "C"
import (
	"encoding/json"
	"net"
	"net/netip"
	"sync"
	_ "tailscale.com/feature/androidbin"
	"tailscale.com/net/netmon"
)

type androidNetwork struct {
	Name      string   `json:"name"`
	MTU       int      `json:"mtu"`
	Addresses []string `json:"addresses"`
	Gateway   string   `json:"gateway"`
}

var androidNetworks struct {
	sync.RWMutex
	raw        string
	interfaces []netmon.Interface
}

func init() {
	netmon.RegisterInterfaceGetter(func() ([]netmon.Interface, error) {
		androidNetworks.RLock()
		defer androidNetworks.RUnlock()
		return append([]netmon.Interface(nil), androidNetworks.interfaces...), nil
	})
}

//export RtcAndroidNetwork
func RtcAndroidNetwork(sd C.int, snapshot *C.char) C.int {
	raw := C.GoString(snapshot)
	androidNetworks.RLock()
	unchanged := androidNetworks.raw == raw
	androidNetworks.RUnlock()
	if unchanged {
		return 0
	}
	var networks []androidNetwork
	if err := json.Unmarshal([]byte(raw), &networks); err != nil {
		return -1
	}
	interfaces := make([]netmon.Interface, 0, len(networks))
	defaultName, gateway := "", ""
	for i, n := range networks {
		if n.Name == "" {
			return -1
		}
		var addresses []net.Addr
		for _, a := range n.Addresses {
			p, err := netip.ParsePrefix(a)
			if err != nil {
				return -1
			}
			addresses = append(addresses, &net.IPNet{IP: p.Addr().AsSlice(), Mask: net.CIDRMask(p.Bits(), p.Addr().BitLen())})
		}
		mtu := n.MTU
		if mtu <= 0 {
			mtu = 1500
		}
		interfaces = append(interfaces, netmon.Interface{Interface: &net.Interface{Index: i + 1, Name: n.Name, MTU: mtu, Flags: net.FlagUp | net.FlagRunning | net.FlagMulticast}, AltAddrs: addresses})
		if i == 0 {
			defaultName, gateway = n.Name, n.Gateway
		}
	}
	androidNetworks.Lock()
	androidNetworks.raw, androidNetworks.interfaces = raw, interfaces
	androidNetworks.Unlock()
	netmon.UpdateLastKnownDefaultRouteInterface(defaultName)
	netmon.UpdateLastKnownDefaultGateway(gateway)
	if s := getServer(sd); s != nil {
		if sys := s.s.Sys(); sys != nil {
			if monitor, ok := sys.NetMon.GetOK(); ok {
				monitor.InjectEvent()
			}
		}
	}
	return 0
}
