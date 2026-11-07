package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/binary"
	"encoding/json"
	"errors"
	"io"
	"net"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"salsa.debian.org/vasudev/gospake2"
)

type lanShortWriter struct {
	bytes.Buffer
	maximum int
}

func (writer *lanShortWriter) Write(data []byte) (int, error) {
	if len(data) > writer.maximum {
		data = data[:writer.maximum]
	}
	return writer.Buffer.Write(data)
}

func TestLanFramesHandlePartialWritesAndRejectUnsafeLengths(t *testing.T) {
	writer := &lanShortWriter{maximum: 1}
	wanted := []byte(`{"op":"list","offset":0}`)
	if err := lanWriteFrame(writer, wanted); err != nil {
		t.Fatal(err)
	}
	got, err := lanReadFrame(&writer.Buffer, lanFrameLimit)
	if err != nil || !bytes.Equal(got, wanted) {
		t.Fatalf("partial frame roundtrip: %q %v", got, err)
	}
	for _, size := range []uint32{0, lanFrameLimit + 1, ^uint32(0)} {
		var header [4]byte
		binary.BigEndian.PutUint32(header[:], size)
		if _, err := lanReadFrame(bytes.NewReader(header[:]), lanFrameLimit); err == nil {
			t.Fatalf("unsafe allocation size %d accepted", size)
		}
	}
	if _, err := lanReadFrame(bytes.NewReader([]byte{0, 0, 0, 3, 1}), lanFrameLimit); err == nil {
		t.Fatal("truncated frame accepted")
	}
	if err := lanWriteAll(&lanShortWriter{maximum: 0}, []byte("x")); !errors.Is(err, io.ErrNoProgress) {
		t.Fatalf("zero-byte writes must stop: %v", err)
	}
}

func TestLanDiscoveryContainsNoPairingSecretAndRejectsWAN(t *testing.T) {
	announcement := []byte(`{"v":1,"id":"session-1","name":"My betterPod","port":45842}`)
	device, ok := lanDiscoveryDevice(announcement,
		&net.UDPAddr{IP: net.ParseIP("192.168.1.20"), Port: lanDiscoveryPort})
	if !ok || device.Host != "192.168.1.20" || device.ID != "session-1" {
		t.Fatalf("local discovery not decoded: %+v", device)
	}
	encoded, _ := json.Marshal(device)
	if strings.Contains(string(encoded), "code") || strings.Contains(lanDiscoveryText, "password") {
		t.Fatal("discovery must not carry the pairing secret")
	}
	for _, host := range []string{"8.8.8.8", "0.0.0.0", "255.255.255.255", "example.com", "224.0.0.1", "::", "2001:4860:4860::8888", "127.0.0.1%en0"} {
		if _, err := lanLocalIP(host); err == nil {
			t.Fatalf("non-local or invalid host accepted: %s", host)
		}
	}
	for _, host := range []string{"127.0.0.1", "192.168.0.4", "10.8.0.9", "172.16.2.3", "169.254.3.4", "::1", "fd12::4", "fe80::1%en0"} {
		if _, err := lanLocalIP(host); err != nil {
			t.Fatalf("local host rejected: %s: %v", host, err)
		}
	}
	for _, source := range []*net.UDPAddr{
		{IP: net.ParseIP("8.8.8.8"), Port: lanDiscoveryPort},
		{IP: net.ParseIP("192.168.1.1"), Port: 12},
	} {
		if _, ok := lanDiscoveryDevice(announcement, source); ok {
			t.Fatal("invalid discovery source accepted")
		}
	}
}

func TestLanDiscoveryFindsLocalDeviceAndDeduplicatesReplies(t *testing.T) {
	socket, err := net.ListenUDP("udp4", &net.UDPAddr{IP: net.IPv4(127, 0, 0, 1), Port: lanDiscoveryPort})
	if err != nil {
		t.Skipf("local discovery listener unavailable in this environment: %v", err)
	}
	defer socket.Close()
	finished := make(chan struct{})
	go func() {
		defer close(finished)
		buffer := make([]byte, 1024)
		for {
			n, source, err := socket.ReadFromUDP(buffer)
			if err != nil {
				return
			}
			if string(buffer[:n]) != lanDiscoveryText {
				continue
			}
			_, _ = socket.WriteToUDP([]byte(`{"v":1,"id":"fixture-discovery","name":"Fixture betterPod","port":45842}`), source)
		}
	}()
	result := lanDiscover()
	_ = socket.Close()
	<-finished
	if result.Error != "" {
		t.Fatal(result.Error)
	}
	count := 0
	for _, device := range result.Devices {
		if device.ID == "fixture-discovery" {
			count++
			if device.Host != "127.0.0.1" || device.Port != 45842 {
				t.Fatalf("discovered wrong endpoint: %+v", device)
			}
		}
	}
	if count != 1 {
		t.Fatalf("discovery should find one deduplicated fixture, got %d: %+v", count, result)
	}
}

func lanTestSession(t *testing.T, conn net.Conn, key []byte, server bool) *lanSession {
	t.Helper()
	sendRole, receiveRole := "client", "server"
	if server {
		sendRole, receiveRole = receiveRole, sendRole
	}
	send, err := lanCipher(key, sendRole)
	if err != nil {
		t.Fatal(err)
	}
	receive, err := lanCipher(key, receiveRole)
	if err != nil {
		t.Fatal(err)
	}
	return &lanSession{conn: conn, send: send, receive: receive}
}

func TestLanEncryptedFramesRejectTamperingReplayAndWrongDirection(t *testing.T) {
	key := sha256.Sum256([]byte("fixture shared secret"))
	for _, attack := range []string{"tamper", "replay", "wrong-direction"} {
		t.Run(attack, func(t *testing.T) {
			left, right := net.Pipe()
			defer left.Close()
			defer right.Close()
			_ = left.SetDeadline(time.Now().Add(time.Second))
			_ = right.SetDeadline(time.Now().Add(time.Second))
			client := lanTestSession(t, left, key[:], false)
			server := lanTestSession(t, right, key[:], true)
			payload := []byte(`{"ok":true}`)
			cipher := server.send
			if attack == "wrong-direction" {
				cipher = server.receive
			}
			frame := cipher.Seal(nil, lanNonce(0), payload, nil)
			if attack == "tamper" {
				frame[len(frame)-1] ^= 1
			}
			written := make(chan error, 1)
			go func() {
				err := lanWriteFrame(right, frame)
				if err == nil && attack == "replay" {
					err = lanWriteFrame(right, frame)
				}
				written <- err
			}()
			_, err := client.receiveJSON()
			if attack == "replay" {
				if err != nil {
					t.Fatal(err)
				}
				_, err = client.receiveJSON()
			}
			if err == nil {
				t.Fatal("unauthenticated frame accepted")
			}
			if err := <-written; err != nil {
				t.Fatal(err)
			}
		})
	}
}

func lanTestServer(t *testing.T, code, sessionID string, handler func([]byte) []byte) (LanConnectRequest, <-chan error) {
	t.Helper()
	listener, err := net.Listen("tcp4", "127.0.0.1:0")
	if err != nil {
		t.Skipf("local TCP listener unavailable in this environment: %v", err)
	}
	t.Cleanup(func() { _ = listener.Close() })
	done := make(chan error, 1)
	go func() {
		conn, err := listener.Accept()
		if err != nil {
			done <- err
			return
		}
		defer conn.Close()
		_ = conn.SetDeadline(time.Now().Add(5 * time.Second))
		incoming, err := lanReadFrame(conn, lanHelloLimit)
		if err != nil {
			done <- err
			return
		}
		var hello lanHello
		if err = json.Unmarshal(incoming, &hello); err != nil {
			done <- err
			return
		}
		spake := gospake2.SPAKE2Symmetric(gospake2.NewPassword(code), gospake2.NewIdentityS(lanIdentity))
		outgoing, _ := json.Marshal(lanHello{Version: 1, Spake: spake.Start()})
		if err = lanWriteFrame(conn, outgoing); err != nil {
			done <- err
			return
		}
		key, err := spake.Finish(hello.Spake)
		if err != nil {
			done <- err
			return
		}
		session := lanTestSession(t, conn, key, true)
		confirmation, err := session.receiveJSON()
		if err != nil {
			done <- err
			return
		}
		var request struct {
			Op   string `json:"op"`
			Name string `json:"name"`
		}
		if json.Unmarshal(confirmation, &request) != nil || request.Op != "confirm" || request.Name != "Fixture Mac" {
			done <- errors.New("invalid confirmation")
			return
		}
		accepted, _ := json.Marshal(map[string]any{"ok": true, "name": "Fixture betterPod", "session": sessionID})
		if err = session.sendJSON(accepted); err != nil {
			done <- err
			return
		}
		for {
			request, err := session.receiveJSON()
			if err != nil {
				done <- err
				return
			}
			response := handler(request)
			if response == nil {
				_, err = io.Copy(io.Discard, conn)
				done <- err
				return
			}
			if err = session.sendJSON(response); err != nil {
				done <- err
				return
			}
		}
	}()
	return LanConnectRequest{Host: "127.0.0.1", Port: listener.Addr().(*net.TCPAddr).Port, Code: code, Name: "Fixture Mac", DeviceID: sessionID}, done
}

func TestLanConnectAuthenticatesCodeAndSelectedDevice(t *testing.T) {
	for _, mode := range []string{"correct", "wrong-code", "changed-device"} {
		t.Run(mode, func(t *testing.T) {
			var operations atomic.Int32
			request, done := lanTestServer(t, "12345678", "fixture-session", func(data []byte) []byte {
				operations.Add(1)
				return []byte(`{"tracks":[],"total":0,"revision":1}`)
			})
			if mode == "wrong-code" {
				request.Code = "87654321"
			} else if mode == "changed-device" {
				request.DeviceID = "old-session"
			} else {
				request.Code = "1234 5678"
			}
			connected := lanConnectRegistered(request)
			if mode != "correct" {
				if connected.ID != 0 || connected.Error == "" {
					t.Fatalf("untrusted pairing accepted: %+v", connected)
				}
				if operations.Load() != 0 {
					t.Fatal("operation reached unauthenticated server")
				}
			} else {
				if connected.Error != "" || connected.ID == 0 || connected.Name != "Fixture betterPod" {
					t.Fatalf("pairing failed: %+v", connected)
				}
				response, err := lanRequest(connected.ID, []byte(`{"op":"list","offset":0,"limit":256}`))
				if err != nil || !bytes.Contains(response, []byte(`"total":0`)) || operations.Load() != 1 {
					t.Fatalf("authenticated operation failed: %s %v", response, err)
				}
				lanDisconnect(connected.ID)
				if _, err = lanRequest(connected.ID, []byte(`{"op":"list"}`)); err == nil {
					t.Fatal("revoked session accepted a request")
				}
			}
			select {
			case <-done:
			case <-time.After(time.Second):
				t.Fatal("pairing server did not close")
			}
		})
	}
}

func TestLanRequestsSerializeAndOperationErrorsKeepSession(t *testing.T) {
	request, done := lanTestServer(t, "12345678", "fixture-session", func(data []byte) []byte {
		var op map[string]json.RawMessage
		_ = json.Unmarshal(data, &op)
		if _, failed := op["fail"]; failed {
			return []byte(`{"error":"The track changed. Reload it"}`)
		}
		return data
	})
	connected := lanConnectRegistered(request)
	if connected.Error != "" {
		t.Fatal(connected.Error)
	}
	t.Cleanup(func() { lanDisconnect(connected.ID) })
	if _, err := lanRequest(connected.ID, []byte(`{"op":"confirm"}`)); err == nil {
		t.Fatal("second confirmation accepted")
	}
	response, err := lanRequest(connected.ID, []byte(`{"op":"cover","fail":true}`))
	if err != nil || !bytes.Contains(response, []byte(`"error"`)) {
		t.Fatalf("operation error lost: %s %v", response, err)
	}
	var workers sync.WaitGroup
	for i := 0; i < 6; i++ {
		workers.Add(1)
		go func(index int) {
			defer workers.Done()
			payload, _ := json.Marshal(map[string]any{"op": "list", "index": index})
			response, err := lanRequest(connected.ID, payload)
			if err != nil || !bytes.Equal(response, payload) {
				t.Errorf("parallel reply mixed with another request: %s %v", response, err)
			}
		}(i)
	}
	workers.Wait()
	lanDisconnect(connected.ID)
	select {
	case <-done:
	case <-time.After(time.Second):
		t.Fatal("session server did not close")
	}
}

func TestLanDisconnectInterruptsPendingRequestWithoutRetry(t *testing.T) {
	received := make(chan struct{}, 1)
	request, serverDone := lanTestServer(t, "12345678", "fixture-session", func(data []byte) []byte {
		received <- struct{}{}
		return nil
	})
	connected := lanConnectRegistered(request)
	if connected.Error != "" {
		t.Fatal(connected.Error)
	}
	requestDone := make(chan error, 1)
	go func() {
		_, err := lanRequest(connected.ID, []byte(`{"op":"save","tracks":[]}`))
		requestDone <- err
	}()
	select {
	case <-received:
	case <-time.After(time.Second):
		t.Fatal("pending request never reached the server")
	}
	lanDisconnect(connected.ID)
	select {
	case err := <-requestDone:
		if err == nil || !strings.Contains(err.Error(), "Reload") {
			t.Fatalf("interrupted save must require reload, not silently retry: %v", err)
		}
	case <-time.After(time.Second):
		t.Fatal("disconnect waited for the request lock")
	}
	select {
	case <-serverDone:
	case <-time.After(time.Second):
		t.Fatal("server remained connected after disconnect")
	}
}
