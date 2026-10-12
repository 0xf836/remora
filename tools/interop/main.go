// Independent test-only CONNECT-UDP peer. No Remora wire codec is imported.
package main

import (
	"bytes"
	"context"
	"crypto/sha256"
	"crypto/subtle"
	"crypto/tls"
	"crypto/x509"
	"encoding/hex"
	"encoding/json"
	"errors"
	"flag"
	"io"
	"log"
	"log/slog"
	"net"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"

	masque "github.com/quic-go/masque-go"
	quic "github.com/quic-go/quic-go"
	"github.com/quic-go/quic-go/http3"
	"github.com/quic-go/quic-go/quicvarint"
	"github.com/yosida95/uritemplate/v3"
)

type config struct {
	Role            string `json:"role"`
	Listen          string `json:"listen"`
	ServerAddress   string `json:"server_address"`
	Authority       string `json:"authority"`
	Target          string `json:"target"`
	ServerName      string `json:"server_name"`
	CAFile          string `json:"ca_file"`
	CertificateFile string `json:"certificate_file"`
	KeyFile         string `json:"key_file"`
	TokenFile       string `json:"token_file"`
	Digest          string `json:"digest"`
	Case            string `json:"case"`
	Flows           int    `json:"flows"`
}

func emit(event string, fields map[string]any) {
	fields["event"] = event
	_ = json.NewEncoder(os.Stdout).Encode(fields)
}
func main() {
	slog.SetDefault(slog.New(slog.NewTextHandler(io.Discard, nil)))
	log.SetOutput(io.Discard) // Third-party arbitrary reasons never enter artifacts.
	path := flag.String("config", "", "private fixture JSON")
	flag.Parse()
	if *path == "" || flag.NArg() != 0 {
		os.Exit(2)
	}
	raw, err := os.ReadFile(*path)
	var c config
	if err == nil {
		d := json.NewDecoder(bytes.NewReader(raw))
		d.DisallowUnknownFields()
		err = d.Decode(&c)
	}
	if err != nil {
		emit("fixture_error", map[string]any{"ok": false})
		os.Exit(2)
	}
	ctx, cancel := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	defer cancel()
	if c.Role == "client" {
		err = client(ctx, c)
	} else if c.Role == "server" {
		err = server(ctx, c)
	} else {
		err = errors.New("role")
	}
	if err != nil {
		emit("peer_failure", map[string]any{"ok": false, "case": c.Case})
		os.Exit(1)
	}
}
func template(c config) (*uritemplate.Template, error) {
	return uritemplate.New("https://" + c.Authority + "/.well-known/masque/udp/{target_host}/{target_port}/")
}
func client(parent context.Context, c config) error {
	ctx, cancel := context.WithTimeout(parent, 10*time.Second)
	defer cancel()
	ca, err := os.ReadFile(c.CAFile)
	if err != nil {
		return err
	}
	roots := x509.NewCertPool()
	if !roots.AppendCertsFromPEM(ca) {
		return errors.New("ca")
	}
	token, err := os.ReadFile(c.TokenFile)
	if err != nil {
		return err
	}
	token = bytes.TrimSuffix(token, []byte("\n"))
	if c.Case == "bad-token" {
		token = bytes.Repeat([]byte("0"), 64)
	}
	tc := &tls.Config{RootCAs: roots, ServerName: c.ServerName, NextProtos: []string{http3.NextProtoH3}, MinVersion: tls.VersionTLS13}
	tr := &masque.Transport{TLSClientConfig: tc, QUICConfig: &quic.Config{EnableDatagrams: true, InitialPacketSize: 1350}, DialAddr: func(ctx context.Context, _ string, t *tls.Config, q *quic.Config) (*quic.Conn, error) {
		return quic.DialAddr(ctx, c.ServerAddress, t, q)
	}}
	tpl, err := template(c)
	if err != nil {
		return err
	}
	// One independent QUIC/H3 connection contains multiple CONNECT-UDP streams.
	q, err := tr.DialAddr(ctx, c.ServerAddress, tc, tr.QUICConfig)
	if err != nil {
		return err
	}
	defer q.CloseWithError(0, "")
	if c.Case == "early-datagram" || c.Case == "malformed-context" || c.Case == "absent-settings" || c.Case == "capsule-request-missing" || c.Case == "capsule-request-oversized" {
		return inject(ctx, q, c, tpl, token)
	}
	if c.Case == "malformed-path" {
		tpl, err = uritemplate.New("https://" + c.Authority + "/not-masque/{target_host}/{target_port}/")
		if err != nil {
			return err
		}
	}
	h3, err := tr.NewClientConn(q)
	if err != nil {
		return err
	}
	flows := c.Flows
	if flows == 0 {
		flows = 2
	}
	if flows < 1 || flows > 32 {
		return errors.New("flows")
	}
	connections := make([]*masque.Conn, 0, flows)
	defer func() {
		for _, p := range connections {
			p.Close()
		}
	}()
	for i := 0; i < flows; i++ {
		req, err := masque.NewRequest(ctx, tpl, c.Target)
		if err != nil {
			return err
		}
		if c.Case != "missing-token" {
			req.Header().Set("Authorization", "Bearer "+string(token))
		}
		p, rsp, err := h3.Dial(req)
		expectedStatus := 0
		switch c.Case {
		case "bad-token", "missing-token", "bad-token-response":
			expectedStatus = http.StatusUnauthorized
		case "denied-target", "mixed-dns":
			expectedStatus = http.StatusForbidden
		case "malformed-path":
			expectedStatus = http.StatusNotFound
		}
		if expectedStatus != 0 {
			if err == nil || rsp == nil || rsp.StatusCode != expectedStatus {
				return errors.New("expected denial")
			}
			emit("denial", map[string]any{"ok": true, "status": rsp.StatusCode, "case": c.Case})
			return nil
		}
		if err != nil {
			return err
		}
		connections = append(connections, p)
	}
	delivered := 0
	for flow, p := range connections {
		for _, size := range []int{0, 1, 128, 512, 1200} {
			data := make([]byte, size)
			for i := range data {
				data[i] = byte((i + flow + size) % 251)
			}
			// The Remora server's conservative ACK receipt may still be pending.
			// Setup probes retry; each numbered body below is sent once after readiness.
			ready := false
			for retry := 0; retry < 20; retry++ {
				if _, err = p.WriteTo([]byte("interop-ready"), p.RemoteAddr()); err != nil {
					return err
				}
				p.SetReadDeadline(time.Now().Add(100 * time.Millisecond))
				var b [65536]byte
				n, _, err := p.ReadFrom(b[:])
				if err == nil && bytes.Equal(b[:n], []byte("interop-ready")) {
					ready = true
					break
				}
			}
			if !ready {
				return errors.New("setup")
			}
			p.SetReadDeadline(time.Now().Add(time.Second))
			if _, err := p.WriteTo(data, p.RemoteAddr()); err != nil {
				return err
			}
			var b [65536]byte
			n, _, err := p.ReadFrom(b[:])
			if err != nil || !bytes.Equal(data, b[:n]) {
				return errors.New("boundary")
			}
			delivered++
		}
	}
	emit("forwarding", map[string]any{"ok": true, "flows": flows, "datagrams": delivered, "sizes": []int{0, 1, 128, 512, 1200}, "independent_stack": true})
	return nil
}
func server(ctx context.Context, c config) error {
	cert, err := tls.LoadX509KeyPair(c.CertificateFile, c.KeyFile)
	if err != nil {
		return err
	}
	digest, err := hex.DecodeString(c.Digest)
	if err != nil || len(digest) != 32 {
		return errors.New("digest")
	}
	tpl, err := template(c)
	if err != nil {
		return err
	}
	var proxy masque.Proxy
	defer proxy.Close()
	handler := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		bearer := r.Header.Get("Authorization")
		valid := len(bearer) == 71 && bearer[:7] == "Bearer "
		if valid {
			for _, b := range []byte(bearer[7:]) {
				if !(b >= '0' && b <= '9' || b >= 'a' && b <= 'f') {
					valid = false
				}
			}
		}
		var hash [32]byte
		if valid {
			hash = sha256.Sum256([]byte(bearer[7:]))
		}
		if !valid || subtle.ConstantTimeCompare(hash[:], digest) != 1 {
			w.WriteHeader(401)
			return
		}
		switch c.Case {
		case "status-204", "status-205", "status-206", "content-length", "content-type", "transfer-encoding", "capsule-missing", "capsule-oversized":
			switch c.Case {
			case "content-length":
				w.Header().Set("Content-Length", "0")
			case "content-type":
				w.Header().Set("Content-Type", "application/octet-stream")
			case "transfer-encoding":
				w.Header().Set("Transfer-Encoding", "chunked")
			}
			status := 200
			switch c.Case {
			case "status-204":
				status = 204
			case "status-205":
				status = 205
			case "status-206":
				status = 206
			}
			w.WriteHeader(status)
			str := w.(http3.HTTPStreamer).HTTPStream()
			if c.Case == "capsule-missing" {
				_ = http3.WriteCapsule(quicvarint.NewWriter(str), 0, nil)
			}
			if c.Case == "capsule-oversized" {
				str.SetWriteDeadline(time.Now().Add(time.Second))
				_ = http3.WriteCapsule(quicvarint.NewWriter(str), 0, make([]byte, 65529))
			}
			<-r.Context().Done()
			return
		}
		req, err := masque.ParseProxyRequest(r, tpl)
		if err != nil {
			w.WriteHeader(400)
			return
		}
		if req.Target != c.Target {
			w.WriteHeader(403)
			return
		}
		// This fixture permits one numeric target only; no independent DNS policy.
		host, _, err := net.SplitHostPort(c.Target)
		if err != nil || net.ParseIP(host) == nil {
			w.WriteHeader(403)
			return
		}
		if c.Case == "hold-response" {
			emit("request_pending", map[string]any{"ok": true})
			select {
			case <-r.Context().Done():
			case <-ctx.Done():
			}
			return
		}
		addr, err := net.ResolveUDPAddr("udp", c.Target)
		if err != nil {
			w.WriteHeader(502)
			return
		}
		conn, err := net.DialUDP("udp", nil, addr)
		if err != nil {
			w.WriteHeader(502)
			return
		}
		_ = proxy.ProxyConnectedSocket(w, req, conn)
	})
	s := http3.Server{Addr: c.Listen, TLSConfig: &tls.Config{Certificates: []tls.Certificate{cert}, MinVersion: tls.VersionTLS13}, QUICConfig: &quic.Config{EnableDatagrams: c.Case != "no-datagram"}, EnableDatagrams: c.Case != "no-datagram", Handler: handler}
	address, err := net.ResolveUDPAddr("udp", c.Listen)
	if err != nil {
		return err
	}
	socket, err := net.ListenUDP("udp", address)
	if err != nil {
		return err
	}
	defer socket.Close()
	done := make(chan error, 1)
	go func() { done <- s.Serve(socket) }()
	emit("listening", map[string]any{"ok": true})
	select {
	case err := <-done:
		return err
	case <-ctx.Done():
		return s.Close()
	}
}

// Injections use independent HTTP/3 stream/capsule and varint APIs. No Remora
// association encoder or QUIC packet code is reused by this fixture.
func inject(ctx context.Context, q *quic.Conn, c config, tpl *uritemplate.Template, token []byte) error {
	tr := &http3.Transport{EnableDatagrams: c.Case != "absent-settings"}
	h3 := tr.NewClientConn(q)
	if c.Case == "absent-settings" {
		// Our SETTINGS disable DATAGRAM. A proxy can refuse that capability
		// before its own SETTINGS reaches us. The runner separately requires
		// exact capability/0 and zero actual target traffic.
		time.Sleep(200 * time.Millisecond)
		emit("drop_injection", map[string]any{"ok": true, "case": c.Case})
		return nil
	}
	select {
	case <-h3.ReceivedSettings():
	case <-ctx.Done():
		return ctx.Err()
	}
	stream, err := h3.OpenRequestStream(ctx)
	if err != nil {
		return err
	}
	if c.Case == "early-datagram" || c.Case == "malformed-context" {
		data := []byte{0xc0}
		if c.Case == "early-datagram" {
			data = append(quicvarint.Append(nil, 0), []byte("INTEROP_PAYLOAD_SENTINEL")...)
		}
		if err := stream.SendDatagram(data); err != nil {
			return err
		}
		time.Sleep(200 * time.Millisecond)
		emit("drop_injection", map[string]any{"ok": true, "case": c.Case})
		return nil
	}
	host, port, err := net.SplitHostPort(c.Target)
	if err != nil {
		return err
	}
	address, err := tpl.Expand(uritemplate.Values{"target_host": uritemplate.String(host), "target_port": uritemplate.String(port)})
	if err != nil {
		return err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodConnect, address, nil)
	if err != nil {
		return err
	}
	req.Proto = "connect-udp"
	req.Header.Set("Authorization", "Bearer "+string(token))
	req.Header.Set("Capsule-Protocol", "?1")
	if err = stream.SendRequestHeader(req); err != nil {
		return err
	}
	stream.SetWriteDeadline(time.Now().Add(time.Second))
	stream.SetReadDeadline(time.Now().Add(2 * time.Second))
	expected := quic.StreamErrorCode(0x10e)
	var body []byte
	if c.Case == "capsule-request-oversized" {
		body = make([]byte, 65529)
		expected = 0x33
	}
	err = http3.WriteCapsule(quicvarint.NewWriter(stream), 0, body)
	check := func(err error) bool {
		var reset *quic.StreamError
		return errors.As(err, &reset) && reset.ErrorCode == expected
	}
	if !check(err) {
		_, err = stream.ReadResponse()
		if err == nil {
			var b [1]byte
			_, err = stream.Read(b[:])
		}
	}
	if !check(err) {
		return errors.New("expected Capsule stream reset")
	}
	emit("protocol_injection", map[string]any{"ok": true, "case": c.Case, "numeric_code": uint64(expected)})
	return nil
}
