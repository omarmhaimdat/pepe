// Target server for the benchmarks in bench/README.md.
//
// It answers every request from memory as fast as Go's net/http can, so that
// the load generator under test, not the server, is the bottleneck.
//
//	go run bench/server.go            # http on 127.0.0.1:8089, https on 127.0.0.1:8090
//	go run bench/server.go -addr :9000 -tls-addr :9443
//
// The HTTPS listener uses a self-signed certificate made at startup, so
// clients need to skip verification (pepe -k, oha --insecure).
//
// Paths:
//
//	/            16-byte body
//	/1k /16k /256k   bodies of that size
//	/json        a small JSON object
//	/slow?ms=20  sleeps before answering
//	/status/503  answers with that status
package main

import (
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"crypto/tls"
	"crypto/x509"
	"crypto/x509/pkix"
	"flag"
	"fmt"
	"math/big"
	"net"
	"net/http"
	"strconv"
	"strings"
	"time"
)

// selfSigned makes a certificate for 127.0.0.1 and localhost, valid for a day
func selfSigned() tls.Certificate {
	key, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		panic(err)
	}
	template := x509.Certificate{
		SerialNumber: big.NewInt(1),
		Subject:      pkix.Name{CommonName: "pepe bench"},
		NotBefore:    time.Now().Add(-time.Hour),
		NotAfter:     time.Now().Add(24 * time.Hour),
		KeyUsage:     x509.KeyUsageDigitalSignature | x509.KeyUsageKeyEncipherment,
		ExtKeyUsage:  []x509.ExtKeyUsage{x509.ExtKeyUsageServerAuth},
		DNSNames:     []string{"localhost"},
		IPAddresses:  []net.IP{net.ParseIP("127.0.0.1"), net.ParseIP("::1")},
	}
	der, err := x509.CreateCertificate(rand.Reader, &template, &template, &key.PublicKey, key)
	if err != nil {
		panic(err)
	}
	return tls.Certificate{Certificate: [][]byte{der}, PrivateKey: key}
}

func main() {
	addr := flag.String("addr", "127.0.0.1:8089", "listen address")
	tlsAddr := flag.String("tls-addr", "127.0.0.1:8090", "HTTPS listen address (empty: none)")
	flag.Parse()

	small := []byte("hello from pepe\n")
	sizes := map[string]string{
		"/1k":   strings.Repeat("x", 1024) + "\n",
		"/16k":  strings.Repeat("x", 16*1024) + "\n",
		"/256k": strings.Repeat("x", 256*1024) + "\n",
	}
	json := []byte(`{"id":42,"name":"pepe","ok":true,"tags":["load","test"]}`)

	mux := http.NewServeMux()
	mux.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "text/plain")
		w.Write(small)
	})
	for path, body := range sizes {
		b := []byte(body)
		mux.HandleFunc(path, func(w http.ResponseWriter, r *http.Request) {
			w.Header().Set("Content-Type", "text/plain")
			w.Write(b)
		})
	}
	mux.HandleFunc("/json", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		w.Header().Set("X-Cache", "HIT")
		w.Write(json)
	})
	mux.HandleFunc("/slow", func(w http.ResponseWriter, r *http.Request) {
		ms, _ := strconv.Atoi(r.URL.Query().Get("ms"))
		if ms <= 0 {
			ms = 10
		}
		time.Sleep(time.Duration(ms) * time.Millisecond)
		w.Write(small)
	})
	mux.HandleFunc("/status/", func(w http.ResponseWriter, r *http.Request) {
		code, err := strconv.Atoi(strings.TrimPrefix(r.URL.Path, "/status/"))
		if err != nil {
			code = 400
		}
		w.WriteHeader(code)
		w.Write(small)
	})

	srv := &http.Server{
		Addr:         *addr,
		Handler:      mux,
		ReadTimeout:  30 * time.Second,
		WriteTimeout: 30 * time.Second,
		IdleTimeout:  120 * time.Second,
	}
	if *tlsAddr != "" {
		tlsSrv := &http.Server{
			Addr:         *tlsAddr,
			Handler:      mux,
			ReadTimeout:  30 * time.Second,
			WriteTimeout: 30 * time.Second,
			IdleTimeout:  120 * time.Second,
			TLSConfig: &tls.Config{
				Certificates: []tls.Certificate{selfSigned()},
				// HTTP/1.1 only, like the plain listener: the load
				// generators under test speak HTTP/1.1
				NextProtos: []string{"http/1.1"},
			},
		}
		go func() {
			fmt.Println("bench server on https://" + *tlsAddr)
			if err := tlsSrv.ListenAndServeTLS("", ""); err != nil {
				panic(err)
			}
		}()
	}
	fmt.Println("bench server on http://" + *addr)
	if err := srv.ListenAndServe(); err != nil {
		panic(err)
	}
}
