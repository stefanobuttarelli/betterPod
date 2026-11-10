package main

// The LAN library client uses an explicit short-lived code for SPAKE2. The
// resulting keys encrypt every request and reply; no relay or HTTP service is
// used. Discovery advertises only the device name, session ID and TCP port.

import (
	"crypto/cipher"
	"crypto/sha256"
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"math"
	"net"
	"sort"
	"strconv"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"golang.org/x/crypto/chacha20poly1305"
	"golang.org/x/crypto/hkdf"
	"salsa.debian.org/vasudev/gospake2"
)

const (
	lanDiscoveryPort = 45841
	lanDiscoveryText = "betterpod-library-discover-v1"
	lanIdentity      = "betterpod.xyz/library/v1"
	lanFrameLimit    = 16 << 20
	lanHelloLimit    = 1024
	lanRequestTime   = 60 * time.Second
)

type LanDevice struct {
	ID   string `json:"id"`
	Name string `json:"name"`
	Host string `json:"host"`
	Port int    `json:"port"`
}

type LanDiscoveryResult struct {
	Devices []LanDevice `json:"devices"`
	Error   string      `json:"error,omitempty"`
}

type LanConnectRequest struct {
	Host     string `json:"host"`
	Port     int    `json:"port"`
	Code     string `json:"code"`
	Name     string `json:"name"`
	DeviceID string `json:"deviceID"`
}

type LanConnectResult struct {
	ID    int    `json:"id,omitempty"`
	Name  string `json:"name,omitempty"`
	Error string `json:"error,omitempty"`
}

type lanHello struct {
	Version int    `json:"v"`
	Spake   []byte `json:"spake"`
	Error   string `json:"error,omitempty"`
}

type lanSession struct {
	conn      net.Conn
	mu        sync.Mutex // One request/reply at a time; Close never takes it.
	closeOnce sync.Once
	send      cipher.AEAD
	receive   cipher.AEAD
	sent      uint64
	received  uint64
}

var lanSessions = struct {
	sync.Mutex
	next int
	byID map[int]*lanSession
}{next: 1, byID: make(map[int]*lanSession)}

func lanLocalIP(host string) (net.IP, error) {
	host = strings.TrimSpace(host)
	address := host
	if index := strings.LastIndexByte(address, '%'); index >= 0 {
		// Zones are valid only for IPv6 link-local addresses.
		if index == len(address)-1 || len(address)-index > 64 {
			return nil, errors.New("Enter a valid local network address")
		}
		address = address[:index]
	}
	ip := net.ParseIP(address)
	if ip == nil || !(ip.IsLoopback() || ip.IsPrivate() || ip.IsLinkLocalUnicast()) {
		return nil, errors.New("Use the betterPod's local Wi-Fi address")
	}
	if address != host && (ip.To4() != nil || !ip.IsLinkLocalUnicast()) {
		return nil, errors.New("Only link-local IPv6 addresses can include a network interface")
	}
	return ip, nil
}

func lanDiscoveryTargets() []*net.UDPAddr {
	targets := []*net.UDPAddr{
		{IP: net.IPv4bcast, Port: lanDiscoveryPort},
		{IP: net.IPv4(127, 0, 0, 1), Port: lanDiscoveryPort},
	}
	seen := map[string]bool{"255.255.255.255": true, "127.0.0.1": true}
	interfaces, _ := net.Interfaces()
	for _, network := range interfaces {
		if network.Flags&net.FlagUp == 0 || network.Flags&net.FlagLoopback != 0 || network.Flags&net.FlagBroadcast == 0 {
			continue
		}
		addresses, _ := network.Addrs()
		for _, address := range addresses {
			ipnet, ok := address.(*net.IPNet)
			if !ok || ipnet.IP.To4() == nil || len(ipnet.Mask) != 4 {
				continue
			}
			ip := ipnet.IP.To4()
			if !(ip.IsPrivate() || ip.IsLinkLocalUnicast()) {
				continue
			}
			broadcast := make(net.IP, 4)
			for i := range broadcast {
				broadcast[i] = ip[i] | ^ipnet.Mask[i]
			}
			if !seen[broadcast.String()] {
				seen[broadcast.String()] = true
				targets = append(targets, &net.UDPAddr{IP: broadcast, Port: lanDiscoveryPort})
			}
			if len(targets) >= 32 {
				return targets
			}
		}
	}
	return targets
}

func lanDiscoveryDevice(data []byte, source *net.UDPAddr) (LanDevice, bool) {
	var response struct {
		Version int    `json:"v"`
		ID      string `json:"id"`
		Name    string `json:"name"`
		Port    int    `json:"port"`
	}
	if len(data) > 2048 || json.Unmarshal(data, &response) != nil || source == nil || source.Port != lanDiscoveryPort ||
		response.Version != 1 || response.Port < 1 || response.Port > 65535 ||
		response.ID == "" || len(response.ID) > 128 || len(response.Name) > 256 {
		return LanDevice{}, false
	}
	host := source.IP.String()
	if _, err := lanLocalIP(host); err != nil {
		return LanDevice{}, false
	}
	name := strings.TrimSpace(response.Name)
	if name == "" {
		name = "betterPod"
	}
	return LanDevice{ID: response.ID, Name: name, Host: host, Port: response.Port}, true
}

func lanDiscover() LanDiscoveryResult {
	out := LanDiscoveryResult{Devices: []LanDevice{}}
	socket, err := net.ListenUDP("udp4", &net.UDPAddr{IP: net.IPv4zero})
	if err != nil {
		out.Error = "Could not search this Wi-Fi network: " + err.Error()
		return out
	}
	defer socket.Close()
	_ = socket.SetReadBuffer(64 * 1024)
	targets := lanDiscoveryTargets()
	deadline := time.Now().Add(2 * time.Second)
	seen := make(map[string]bool)
	buffer := make([]byte, 2049)
	for time.Now().Before(deadline) {
		for _, target := range targets {
			_, _ = socket.WriteToUDP([]byte(lanDiscoveryText), target)
		}
		until := time.Now().Add(650 * time.Millisecond)
		if until.After(deadline) {
			until = deadline
		}
		_ = socket.SetReadDeadline(until)
		for {
			n, source, err := socket.ReadFromUDP(buffer)
			if err != nil {
				if timeout, ok := err.(net.Error); ok && timeout.Timeout() {
					break
				}
				out.Error = "The Wi-Fi search stopped: " + err.Error()
				return out
			}
			device, valid := lanDiscoveryDevice(buffer[:n], source)
			key := device.Host + ":" + strconv.Itoa(device.Port) + "/" + device.ID
			if valid && !seen[key] && len(out.Devices) < 64 {
				seen[key] = true
				out.Devices = append(out.Devices, device)
			}
		}
	}
	sort.Slice(out.Devices, func(i, j int) bool {
		a, b := out.Devices[i], out.Devices[j]
		if a.Name != b.Name {
			return strings.ToLower(a.Name) < strings.ToLower(b.Name)
		}
		return a.Host < b.Host
	})
	return out
}

func lanReadFrame(reader io.Reader, limit int) ([]byte, error) {
	var header [4]byte
	if _, err := io.ReadFull(reader, header[:]); err != nil {
		return nil, err
	}
	size := binary.BigEndian.Uint32(header[:])
	if size == 0 || uint64(size) > uint64(limit) {
		return nil, errors.New("The betterPod sent an invalid message size")
	}
	data := make([]byte, int(size))
	_, err := io.ReadFull(reader, data)
	return data, err
}

func lanWriteAll(writer io.Writer, data []byte) error {
	for len(data) > 0 {
		n, err := writer.Write(data)
		if n < 0 || n > len(data) {
			return errors.New("Invalid network write count")
		}
		data = data[n:]
		if err != nil {
			return err
		}
		if n == 0 {
			return io.ErrNoProgress
		}
	}
	return nil
}

func lanWriteFrame(writer io.Writer, data []byte) error {
	if len(data) == 0 || len(data) > lanFrameLimit {
		return errors.New("This request is too large. Save fewer tracks at a time")
	}
	var header [4]byte
	binary.BigEndian.PutUint32(header[:], uint32(len(data)))
	if err := lanWriteAll(writer, header[:]); err != nil {
		return err
	}
	return lanWriteAll(writer, data)
}

func lanCipher(key []byte, role string) (cipher.AEAD, error) {
	derived := make([]byte, chacha20poly1305.KeySize)
	if _, err := io.ReadFull(hkdf.New(sha256.New, key, nil, []byte("betterpod-library-v1/"+role)), derived); err != nil {
		return nil, err
	}
	aead, err := chacha20poly1305.New(derived)
	for i := range derived {
		derived[i] = 0
	}
	return aead, err
}

func lanNonce(counter uint64) []byte {
	var nonce [chacha20poly1305.NonceSize]byte
	binary.BigEndian.PutUint64(nonce[4:], counter)
	return nonce[:]
}

func (session *lanSession) close() {
	session.closeOnce.Do(func() { _ = session.conn.Close() })
}

func (session *lanSession) sendJSON(data []byte) error {
	if len(data) == 0 || len(data) > lanFrameLimit-session.send.Overhead() || !json.Valid(data) {
		return errors.New("The request is invalid or too large. Save fewer tracks at a time")
	}
	if session.sent == math.MaxUint64 {
		return errors.New("The Wi-Fi session has expired. Connect again")
	}
	encrypted := session.send.Seal(nil, lanNonce(session.sent), data, nil)
	if err := lanWriteFrame(session.conn, encrypted); err != nil {
		return err
	}
	session.sent++
	return nil
}

func (session *lanSession) receiveJSON() ([]byte, error) {
	if session.received == math.MaxUint64 {
		return nil, errors.New("The Wi-Fi session has expired. Connect again")
	}
	encrypted, err := lanReadFrame(session.conn, lanFrameLimit)
	if err != nil {
		return nil, err
	}
	data, err := session.receive.Open(nil, lanNonce(session.received), encrypted, nil)
	if err != nil {
		return nil, errors.New("The encrypted Wi-Fi reply could not be verified")
	}
	if !json.Valid(data) {
		return nil, errors.New("The betterPod sent an unreadable reply")
	}
	session.received++
	return data, nil
}

func lanConnect(request LanConnectRequest) (*lanSession, string, error) {
	request.Host = strings.TrimSpace(request.Host)
	if _, err := lanLocalIP(request.Host); err != nil {
		return nil, "", err
	}
	if request.Port < 1 || request.Port > 65535 || len(request.DeviceID) > 128 {
		return nil, "", errors.New("Choose a valid betterPod address and port")
	}
	code := strings.NewReplacer(" ", "", "-", "").Replace(strings.TrimSpace(request.Code))
	if len(code) != 8 || strings.IndexFunc(code, func(r rune) bool { return r < '0' || r > '9' }) >= 0 {
		return nil, "", errors.New("Enter the eight-digit code shown on betterPod")
	}
	name := strings.TrimSpace(request.Name)
	if !utf8.ValidString(name) || len(name) > 128 {
		return nil, "", errors.New("Use a Mac name shorter than 129 bytes")
	}
	if name == "" {
		name = "Mac"
	}
	conn, err := net.DialTimeout("tcp", net.JoinHostPort(request.Host, strconv.Itoa(request.Port)), 3*time.Second)
	if err != nil {
		return nil, "", fmt.Errorf("Could not reach betterPod on Wi-Fi: %w", err)
	}
	session := &lanSession{conn: conn}
	succeeded := false
	defer func() {
		if !succeeded {
			session.close()
		}
	}()
	_ = conn.SetDeadline(time.Now().Add(8 * time.Second))
	spake := gospake2.SPAKE2Symmetric(gospake2.NewPassword(code), gospake2.NewIdentityS(lanIdentity))
	hello, _ := json.Marshal(lanHello{Version: 1, Spake: spake.Start()})
	if err := lanWriteFrame(conn, hello); err != nil {
		return nil, "", errors.New("The Wi-Fi connection stopped during pairing")
	}
	response, err := lanReadFrame(conn, lanHelloLimit)
	if err != nil {
		return nil, "", errors.New("betterPod did not answer the pairing request. Check its Library on Mac screen")
	}
	var reply lanHello
	if json.Unmarshal(response, &reply) != nil {
		return nil, "", errors.New("This betterPod uses an unsupported Wi-Fi protocol")
	}
	if reply.Error != "" {
		return nil, "", errors.New("betterPod could not start pairing: " + lanShortError(reply.Error))
	}
	if reply.Version != 1 || len(reply.Spake) != 33 {
		return nil, "", errors.New("This betterPod uses an unsupported Wi-Fi protocol")
	}
	key, err := spake.Finish(reply.Spake)
	if err != nil {
		return nil, "", errors.New("The betterPod pairing message could not be verified")
	}
	session.send, err = lanCipher(key, "client")
	if err == nil {
		session.receive, err = lanCipher(key, "server")
	}
	for i := range key {
		key[i] = 0
	}
	if err != nil {
		return nil, "", errors.New("The encrypted Wi-Fi session could not be prepared")
	}
	confirmation, _ := json.Marshal(map[string]string{"op": "confirm", "name": name})
	if err := session.sendJSON(confirmation); err != nil {
		return nil, "", errors.New("The Wi-Fi connection stopped while checking the code")
	}
	confirmation, err = session.receiveJSON()
	if err != nil {
		return nil, "", errors.New("The code was not accepted. Check the code on betterPod and try again")
	}
	var accepted struct {
		OK      bool   `json:"ok"`
		Name    string `json:"name"`
		Session string `json:"session"`
		Error   string `json:"error"`
	}
	if json.Unmarshal(confirmation, &accepted) != nil || !accepted.OK || accepted.Session == "" ||
		(request.DeviceID != "" && request.DeviceID != accepted.Session) {
		return nil, "", errors.New("The selected betterPod session changed. Search again and use its new code")
	}
	if len(accepted.Name) > 256 || len(accepted.Session) > 128 {
		return nil, "", errors.New("The betterPod sent invalid pairing details")
	}
	if accepted.Name == "" {
		accepted.Name = "betterPod"
	}
	_ = conn.SetDeadline(time.Time{})
	succeeded = true
	return session, accepted.Name, nil
}

func lanShortError(message string) string {
	if len(message) > 512 {
		return "The device is not ready. Stop and restart Library on Mac"
	}
	return message
}

func lanRegister(session *lanSession) (int, error) {
	lanSessions.Lock()
	defer lanSessions.Unlock()
	if len(lanSessions.byID) >= 8 {
		return 0, errors.New("Too many Wi-Fi sessions are open. Disconnect a device first")
	}
	id := lanSessions.next
	lanSessions.next++
	if lanSessions.next >= math.MaxInt32 {
		lanSessions.next = 1
	}
	for lanSessions.byID[id] != nil {
		id = lanSessions.next
		lanSessions.next++
	}
	lanSessions.byID[id] = session
	return id, nil
}

func lanConnectRegistered(request LanConnectRequest) LanConnectResult {
	session, name, err := lanConnect(request)
	if err != nil {
		return LanConnectResult{Error: err.Error()}
	}
	id, err := lanRegister(session)
	if err != nil {
		session.close()
		return LanConnectResult{Error: err.Error()}
	}
	return LanConnectResult{ID: id, Name: name}
}

func lanDisconnect(id int) {
	lanSessions.Lock()
	session := lanSessions.byID[id]
	delete(lanSessions.byID, id)
	lanSessions.Unlock()
	if session != nil {
		session.close()
	}
}

func lanRequest(id int, data []byte) ([]byte, error) {
	lanSessions.Lock()
	session := lanSessions.byID[id]
	lanSessions.Unlock()
	if session == nil {
		return nil, errors.New("This Wi-Fi session has ended. Connect to betterPod again")
	}
	// Check before acquiring the connection: malformed calls cannot desynchronize
	// an otherwise valid authenticated session.
	if len(data) > lanFrameLimit-chacha20poly1305.Overhead || !json.Valid(data) {
		return nil, errors.New("The request is invalid or too large. Save fewer tracks at a time")
	}
	var operation struct {
		Op string `json:"op"`
	}
	if json.Unmarshal(data, &operation) != nil || operation.Op == "" || operation.Op == "confirm" {
		return nil, errors.New("Choose a valid library operation")
	}
	session.mu.Lock()
	defer session.mu.Unlock()
	_ = session.conn.SetDeadline(time.Now().Add(lanRequestTime))
	if err := session.sendJSON(data); err != nil {
		lanDisconnect(id)
		return nil, fmt.Errorf("The Wi-Fi request stopped. Reload the library before retrying a save: %w", err)
	}
	reply, err := session.receiveJSON()
	if err != nil {
		lanDisconnect(id)
		return nil, fmt.Errorf("The Wi-Fi reply stopped. Reload the library before retrying a save: %w", err)
	}
	_ = session.conn.SetDeadline(time.Time{})
	return reply, nil
}
