package main

import (
	"context"
	"flag"
	"fmt"
	"io"
	"log"
	"net"
	"os"
	"os/signal"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"syscall"
	"time"

	"tailscale.com/tsnet"
)

type forwardRule struct {
	remotePort int
	localAddr  string
}

func parseForwards(spec string) ([]forwardRule, error) {
	var rules []forwardRule
	parts := strings.Split(spec, ",")
	for _, part := range parts {
		part = strings.TrimSpace(part)
		if part == "" {
			continue
		}
		chunks := strings.Split(part, ":")
		if len(chunks) == 2 {
			port, err := strconv.Atoi(chunks[0])
			if err != nil {
				return nil, fmt.Errorf("invalid port %s: %w", chunks[0], err)
			}
			rules = append(rules, forwardRule{
				remotePort: port,
				localAddr:  fmt.Sprintf("127.0.0.1:%s", chunks[1]),
			})
		} else if len(chunks) == 3 {
			port, err := strconv.Atoi(chunks[0])
			if err != nil {
				return nil, fmt.Errorf("invalid port %s: %w", chunks[0], err)
			}
			rules = append(rules, forwardRule{
				remotePort: port,
				localAddr:  fmt.Sprintf("%s:%s", chunks[1], chunks[2]),
			})
		} else {
			return nil, fmt.Errorf("invalid forward spec '%s', expected 'remotePort:localPort' or 'remotePort:host:localPort'", part)
		}
	}
	return rules, nil
}

func handleProxy(remote net.Conn, localAddr string) {
	defer remote.Close()
	local, err := net.DialTimeout("tcp", localAddr, 5*time.Second)
	if err != nil {
		log.Printf("proxy dial %s failed: %v", localAddr, err)
		return
	}
	defer local.Close()

	var wg sync.WaitGroup
	wg.Add(2)

	go func() {
		defer wg.Done()
		_, _ = io.Copy(local, remote)
		if tc, ok := local.(*net.TCPConn); ok {
			_ = tc.CloseWrite()
		}
	}()

	go func() {
		defer wg.Done()
		_, _ = io.Copy(remote, local)
		if tc, ok := remote.(*net.TCPConn); ok {
			_ = tc.CloseWrite()
		}
	}()

	wg.Wait()
}

func main() {
	authKey := flag.String("authkey", os.Getenv("TS_AUTHKEY"), "Tailscale auth key")
	hostname := flag.String("hostname", "", "Hostname on Tailscale network")
	stateDir := flag.String("dir", "", "State directory")
	forwardSpec := flag.String("forward", "8100:8100,9001:9001,9200:9200", "Ports to forward: remotePort:localPort,...")
	ephemeral := flag.Bool("ephemeral", true, "Register as ephemeral node (auto-deregisters on exit)")
	verbose := flag.Bool("v", false, "Verbose logging")
	flag.Parse()

	if *authKey == "" {
		log.Fatal("missing -authkey or TS_AUTHKEY environment variable")
	}

	if *hostname == "" {
		host, _ := os.Hostname()
		if host == "" {
			host = "worker"
		}
		*hostname = fmt.Sprintf("meridian-%s", strings.ToLower(host))
	}

	if *stateDir == "" {
		home, _ := os.UserHomeDir()
		*stateDir = filepath.Join(home, ".local", "state", "meridian", "tsnet")
	}
	if err := os.MkdirAll(*stateDir, 0700); err != nil {
		log.Fatalf("failed to create state directory: %v", err)
	}

	rules, err := parseForwards(*forwardSpec)
	if err != nil {
		log.Fatalf("failed to parse forward rules: %v", err)
	}

	s := &tsnet.Server{
		Hostname:  *hostname,
		AuthKey:   *authKey,
		Dir:       *stateDir,
		Ephemeral: *ephemeral,
	}
	if !*verbose {
		s.Logf = func(format string, args ...any) {
			// Suppress noisy internal tsnet debug logs unless verbose
			msg := fmt.Sprintf(format, args...)
			if strings.Contains(msg, "health") || strings.Contains(msg, "magicsock") {
				return
			}
		}
	}

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	sigCh := make(chan os.Signal, 1)
	signal.Notify(sigCh, os.Interrupt, syscall.SIGTERM)
	go func() {
		<-sigCh
		log.Println("shutting down meridian-mesh...")
		cancel()
		s.Close()
		os.Exit(0)
	}()

	log.Printf("connecting to Tailnet as %s (ephemeral=%v)...", *hostname, *ephemeral)
	
	// Wait for tsnet to connect and get local client status
	lc, err := s.LocalClient()
	if err != nil {
		log.Fatalf("failed to get local client: %v", err)
	}

	// Start listeners on the Tailnet virtual network
	for _, rule := range rules {
		rule := rule
		ln, err := s.Listen("tcp", fmt.Sprintf(":%d", rule.remotePort))
		if err != nil {
			log.Fatalf("failed to listen on Tailnet port %d: %v", rule.remotePort, err)
		}
		log.Printf("✓ Forwarding Tailnet :%d -> %s", rule.remotePort, rule.localAddr)

		go func() {
			for {
				conn, err := ln.Accept()
				if err != nil {
					select {
					case <-ctx.Done():
						return
					default:
						log.Printf("accept error on port %d: %v", rule.remotePort, err)
						return
					}
				}
				go handleProxy(conn, rule.localAddr)
			}
		}()
	}

	// Fetch assigned Tailscale IP
	go func() {
		for i := 0; i < 30; i++ {
			time.Sleep(1 * time.Second)
			st, err := lc.Status(ctx)
			if err == nil && st.Self != nil && len(st.Self.TailscaleIPs) > 0 {
				log.Printf("==================================================")
				log.Printf("🚀 Meridian Mesh Online!")
				log.Printf("Tailscale IP:   %s", st.Self.TailscaleIPs[0])
				log.Printf("Tailscale Host: %s", st.Self.DNSName)
				log.Printf("==================================================")
				return
			}
		}
	}()

	// Keep running
	select {}
}
