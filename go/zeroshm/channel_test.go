package zeroshm

import (
	"fmt"
	"os"
	"sync"
	"testing"
	"time"
)

func setupChannelTest[T Numeric](t *testing.T, namePrefix string, capacity int) (*Memory, *Channel[T]) {
	t.Helper()
	name := fmt.Sprintf("/chan_%s_%d", namePrefix, os.Getpid())
	UnlinkName(name)
	t.Cleanup(func() { UnlinkName(name) })

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	t.Cleanup(func() { mem.Close() })

	ch, err := NewChannel[T](mem, "chan_"+namePrefix, capacity)
	if err != nil {
		t.Fatalf("NewChannel failed: %v", err)
	}
	t.Cleanup(func() { ch.Destroy() })

	return mem, ch
}

// --- Buffered Channel Tests ---

func TestChannelBufferedName(t *testing.T) {
	_, ch := setupChannelTest[int32](t, "name", 16)
	if ch.Name() != "chan_name" {
		t.Errorf("expected name 'chan_name', got %q", ch.Name())
	}
}

func TestChannelBufferedCreate(t *testing.T) {
	_, ch := setupChannelTest[int32](t, "create", 16)
	if ch.Capacity() != 16 {
		t.Errorf("expected capacity 16, got %d", ch.Capacity())
	}
	if !ch.IsBuffered() {
		t.Error("expected channel to be buffered")
	}
	if ch.IsClosed() {
		t.Error("expected channel to not be closed")
	}
}

func TestChannelBufferedSendReceive(t *testing.T) {
	_, ch := setupChannelTest[int32](t, "sendrecv", 16)

	if !ch.Send(42) {
		t.Fatal("Send failed")
	}

	val, ok := ch.Receive()
	if !ok {
		t.Fatal("Receive failed")
	}
	if val != 42 {
		t.Errorf("expected value 42, got %d", val)
	}
}

func TestChannelBufferedFIFOOrder(t *testing.T) {
	_, ch := setupChannelTest[int32](t, "fifo", 16)

	for i := 1; i <= 5; i++ {
		if !ch.Send(int32(i * 10)) {
			t.Fatalf("Send %d failed", i)
		}
	}

	for i := 1; i <= 5; i++ {
		val, ok := ch.Receive()
		if !ok {
			t.Fatalf("Receive %d failed", i)
		}
		if val != int32(i*10) {
			t.Errorf("expected %d, got %d", i*10, val)
		}
	}
}

func TestChannelBufferedTrySendFull(t *testing.T) {
	_, ch := setupChannelTest[int32](t, "tryfull", 4)

	for i := 0; i < 4; i++ {
		if err := ch.TrySend(int32(i)); err != nil {
			t.Fatalf("TrySend %d failed: %v", i, err)
		}
	}

	if err := ch.TrySend(10); err != ChannelFull {
		t.Errorf("expected ChannelFull, got %v", err)
	}
}

func TestChannelBufferedTryReceiveEmpty(t *testing.T) {
	_, ch := setupChannelTest[int32](t, "tryempty", 16)

	_, err := ch.TryReceive()
	if err != ChannelEmpty {
		t.Errorf("expected ChannelEmpty, got %v", err)
	}
}

func TestChannelBufferedReceiveTimeout(t *testing.T) {
	_, ch := setupChannelTest[int32](t, "recvtimeout", 16)

	// Timeout with no data
	_, ok := ch.ReceiveTimeout(50 * time.Millisecond)
	if ok {
		t.Error("expected timeout, got value")
	}

	// Send then receive with timeout
	ch.Send(99)
	val, ok := ch.ReceiveTimeout(100 * time.Millisecond)
	if !ok {
		t.Error("expected value, got timeout")
	}
	if val != 99 {
		t.Errorf("expected 99, got %d", val)
	}
}

func TestChannelBufferedSendTimeout(t *testing.T) {
	_, ch := setupChannelTest[int32](t, "sendtimeout", 2)

	// Fill the buffer
	ch.Send(1)
	ch.Send(2)

	// Send with short timeout should fail (buffer full)
	if ch.SendTimeout(3, 50*time.Millisecond) {
		t.Error("expected send timeout to fail")
	}
}

func TestChannelBufferedClose(t *testing.T) {
	_, ch := setupChannelTest[int32](t, "close", 16)

	ch.Send(1)
	ch.Send(2)
	ch.Close()

	if !ch.IsClosed() {
		t.Error("expected channel to be closed")
	}

	// Can still drain remaining data
	val, ok := ch.Receive()
	if !ok || val != 1 {
		t.Errorf("expected value 1, got %d, ok=%v", val, ok)
	}
	val, ok = ch.Receive()
	if !ok || val != 2 {
		t.Errorf("expected value 2, got %d, ok=%v", val, ok)
	}

	// After drain, TryReceive should return ChannelClosed
	_, err := ch.TryReceive()
	if err != ChannelClosed {
		t.Errorf("expected ChannelClosed, got %v", err)
	}

	// Send on closed channel should fail
	if ch.Send(3) {
		t.Error("expected Send on closed channel to fail")
	}
}

func TestChannelBufferedOpenCrossProcess(t *testing.T) {
	_, ch := setupChannelTest[int32](t, "crossproc", 16)

	ch.Send(100)
	ch.Send(200)

	ch2, err := OpenChannel[int32](ch.memory, "chan_crossproc")
	if err != nil {
		t.Fatalf("OpenChannel failed: %v", err)
	}
	defer ch2.Destroy()

	val, ok := ch2.Receive()
	if !ok || val != 100 {
		t.Errorf("expected 100, got %d, ok=%v", val, ok)
	}
	val, ok = ch2.Receive()
	if !ok || val != 200 {
		t.Errorf("expected 200, got %d, ok=%v", val, ok)
	}
}

func TestChannelBufferedOpenNotFound(t *testing.T) {
	name := fmt.Sprintf("/chan_notfound_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()

	_, err = OpenChannel[int32](mem, "nonexistent")
	if err == nil {
		t.Error("expected error for non-existent channel")
	}
}

func TestChannelNewNameTooLong(t *testing.T) {
	name := fmt.Sprintf("/chan_longname_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()

	longName := ""
	for i := 0; i < 300; i++ {
		longName += "a"
	}
	_, err = NewChannel[int32](mem, longName, 16)
	if err == nil {
		t.Error("expected error for name too long")
	}
}

func TestChannelNewNameTooShort(t *testing.T) {
	name := fmt.Sprintf("/chan_shortname_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()

	_, err = NewChannel[int32](mem, "ab", 16)
	if err == nil {
		t.Error("expected error for name too short")
	}
}

func TestChannelNewAlreadyExists(t *testing.T) {
	_, ch := setupChannelTest[int32](t, "dup", 16)
	_ = ch

	_, err := NewChannel[int32](ch.memory, "chan_dup", 16)
	if err == nil {
		t.Error("expected error for duplicate name")
	}
}

func TestChannelBufferedCapacityRounding(t *testing.T) {
	_, ch := setupChannelTest[int32](t, "round", 3)
	// 3 should round up to 4
	if ch.Capacity() != 4 {
		t.Errorf("expected capacity 4 (rounded from 3), got %d", ch.Capacity())
	}
}

func TestChannelBufferedMPMC(t *testing.T) {
	_, ch := setupChannelTest[int32](t, "mpmc", 64)

	var wg sync.WaitGroup
	const numSenders = 4
	const numReceivers = 4
	const msgsPerSender = 16

	// Multiple senders
	for i := 0; i < numSenders; i++ {
		wg.Add(1)
		go func(senderID int) {
			defer wg.Done()
			for j := 0; j < msgsPerSender; j++ {
				for {
					if ch.Send(int32(senderID*100 + j)) {
						break
					}
					time.Sleep(time.Microsecond)
				}
			}
		}(i)
	}

	// Multiple receivers
	totalReceived := 0
	var mu sync.Mutex
	for i := 0; i < numReceivers; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for {
				val, err := ch.TryReceive()
				if err == ChannelEmpty {
					time.Sleep(time.Microsecond)
					continue
				}
				if err == ChannelClosed {
					return
				}
				mu.Lock()
				totalReceived++
				mu.Unlock()
				_ = val
			}
		}()
	}

	// Wait for all senders to finish, then close
	go func() {
		var wgSenders sync.WaitGroup
		wgSenders.Add(numSenders)
		// We can't reuse the same wg, so wait a bit and close
		time.Sleep(500 * time.Millisecond)
		ch.Close()
	}()

	wg.Wait()

	expected := numSenders * msgsPerSender
	if totalReceived != expected {
		t.Errorf("expected %d messages received, got %d", expected, totalReceived)
	}
}

// --- Unbuffered Channel Tests ---

func TestChannelUnbufferedCreate(t *testing.T) {
	_, ch := setupChannelTest[int32](t, "unbuf", 0)
	if ch.Capacity() != 0 {
		t.Errorf("expected capacity 0, got %d", ch.Capacity())
	}
	if ch.IsBuffered() {
		t.Error("expected channel to not be buffered")
	}
}

func TestChannelUnbufferedRendezvous(t *testing.T) {
	_, ch := setupChannelTest[int32](t, "rendezvous", 0)

	var wg sync.WaitGroup
	var received int32

	// Receiver blocks until sender sends
	wg.Add(1)
	go func() {
		defer wg.Done()
		val, ok := ch.Receive()
		if !ok {
			t.Error("Receive failed")
			return
		}
		received = val
	}()

	// Give receiver time to start waiting
	time.Sleep(10 * time.Millisecond)

	// Sender blocks until receiver receives
	if !ch.Send(42) {
		t.Error("Send failed")
	}

	wg.Wait()

	if received != 42 {
		t.Errorf("expected 42, got %d", received)
	}
}

func TestChannelUnbufferedTrySendNoReceiver(t *testing.T) {
	_, ch := setupChannelTest[int32](t, "trysendnorecv", 0)

	if err := ch.TrySend(42); err != ChannelFull {
		t.Errorf("expected ChannelFull (no receiver waiting), got %v", err)
	}
}

func TestChannelUnbufferedTryReceiveNoSender(t *testing.T) {
	_, ch := setupChannelTest[int32](t, "tryrecvnosend", 0)

	_, err := ch.TryReceive()
	if err != ChannelEmpty {
		t.Errorf("expected ChannelEmpty, got %v", err)
	}
}

func TestChannelUnbufferedCloseUnblocks(t *testing.T) {
	_, ch := setupChannelTest[int32](t, "closeunblock", 0)

	var wg sync.WaitGroup

	// Receiver blocks
	wg.Add(1)
	go func() {
		defer wg.Done()
		_, _ = ch.Receive()
	}()

	// Give receiver time to start waiting
	time.Sleep(10 * time.Millisecond)

	// Close should unblock the receiver
	ch.Close()

	wg.Wait()
}

func TestChannelInt64(t *testing.T) {
	_, ch := setupChannelTest[int64](t, "int64", 8)

	ch.Send(int64(1234567890123))
	val, ok := ch.Receive()
	if !ok {
		t.Fatal("Receive failed")
	}
	if val != int64(1234567890123) {
		t.Errorf("expected 1234567890123, got %d", val)
	}
}

func TestChannelFloat64(t *testing.T) {
	_, ch := setupChannelTest[float64](t, "float64", 8)

	ch.Send(3.14159)
	val, ok := ch.Receive()
	if !ok {
		t.Fatal("Receive failed")
	}
	if val < 3.14158 || val > 3.14160 {
		t.Errorf("expected ~3.14159, got %f", val)
	}
}

func TestChannelDestroyAndReuse(t *testing.T) {
	name := fmt.Sprintf("/chan_destroy_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()

	ch, err := NewChannel[int32](mem, "chan_destroy", 16)
	if err != nil {
		t.Fatalf("NewChannel failed: %v", err)
	}
	ch.Destroy()

	// Should be able to create again
	ch2, err := NewChannel[int32](mem, "chan_destroy", 16)
	if err != nil {
		t.Fatalf("NewChannel after destroy failed: %v", err)
	}
	ch2.Destroy()
}
