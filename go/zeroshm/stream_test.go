package zeroshm

import (
	"fmt"
	"os"
	"testing"
	"time"
)

func TestStreamName(t *testing.T) {
	name := fmt.Sprintf("/stream_name_%d", os.Getpid())
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	s, err := NewStream[int32](mem, "strm", 10)
	if err != nil {
		t.Fatalf("NewStream failed: %v", err)
	}

	if s.Name() != "strm" {
		t.Errorf("Name() = %q, want %q", s.Name(), "strm")
	}
}

func TestStreamNewOpen(t *testing.T) {
	name := fmt.Sprintf("/stream_open_%d", os.Getpid())
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	s, err := NewStream[int32](mem, "strm", 10)
	if err != nil {
		t.Fatalf("NewStream failed: %v", err)
	}

	if s.Capacity() != 10 {
		t.Errorf("Capacity() = %d, want 10", s.Capacity())
	}

	// Open the same stream
	s2, err := OpenStream[int32](mem, "strm")
	if err != nil {
		t.Fatalf("OpenStream failed: %v", err)
	}

	if s2.Capacity() != 10 {
		t.Errorf("OpenStream Capacity() = %d, want 10", s2.Capacity())
	}
}

func TestStreamEmitRead(t *testing.T) {
	name := fmt.Sprintf("/stream_emitread_%d", os.Getpid())
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	s, err := NewStream[int32](mem, "strm", 10)
	if err != nil {
		t.Fatalf("NewStream failed: %v", err)
	}

	if !s.Emit(42) {
		t.Fatal("Emit(42) failed")
	}

	value, ok := s.Read()
	if !ok {
		t.Fatal("Read() failed")
	}
	if value != 42 {
		t.Errorf("Read() = %d, want 42", value)
	}
}

func TestStreamEmitMultiple(t *testing.T) {
	name := fmt.Sprintf("/stream_emitmulti_%d", os.Getpid())
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	s, err := NewStream[int32](mem, "strm", 10)
	if err != nil {
		t.Fatalf("NewStream failed: %v", err)
	}

	// Emit 5 values
	for i := int32(0); i < 5; i++ {
		if !s.Emit(i * 10) {
			t.Fatalf("Emit(%d) failed", i*10)
		}
	}

	// Read them back in order
	for i := int32(0); i < 5; i++ {
		value, ok := s.Read()
		if !ok {
			t.Fatalf("Read() at %d failed", i)
		}
		if value != i*10 {
			t.Errorf("Read() at %d = %d, want %d", i, value, i*10)
		}
	}

	// Should be empty now
	_, ok := s.Read()
	if ok {
		t.Error("Read() on empty stream should return false")
	}
}

func TestStreamReadEmpty(t *testing.T) {
	name := fmt.Sprintf("/stream_readempty_%d", os.Getpid())
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	s, err := NewStream[int32](mem, "strm", 10)
	if err != nil {
		t.Fatalf("NewStream failed: %v", err)
	}

	value, ok := s.Read()
	if ok {
		t.Errorf("Read() on empty stream should return false, got %d", value)
	}
}

func TestStreamCapacity(t *testing.T) {
	name := fmt.Sprintf("/stream_capacity_%d", os.Getpid())
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	s, err := NewStream[int32](mem, "strm", 25)
	if err != nil {
		t.Fatalf("NewStream failed: %v", err)
	}

	if s.Capacity() != 25 {
		t.Errorf("Capacity() = %d, want 25", s.Capacity())
	}
}

func TestStreamSize(t *testing.T) {
	name := fmt.Sprintf("/stream_size_%d", os.Getpid())
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	s, err := NewStream[int32](mem, "strm", 10)
	if err != nil {
		t.Fatalf("NewStream failed: %v", err)
	}

	if s.Size() != 0 {
		t.Errorf("Size() = %d, want 0", s.Size())
	}

	s.Emit(1)
	s.Emit(2)
	s.Emit(3)

	if s.Size() != 3 {
		t.Errorf("Size() = %d, want 3", s.Size())
	}

	s.Read()
	s.Read()

	if s.Size() != 1 {
		t.Errorf("Size() = %d, want 1", s.Size())
	}
}

func TestStreamVersion(t *testing.T) {
	name := fmt.Sprintf("/stream_version_%d", os.Getpid())
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	s, err := NewStream[int32](mem, "strm", 10)
	if err != nil {
		t.Fatalf("NewStream failed: %v", err)
	}

	if s.Version() != 0 {
		t.Errorf("Version() = %d, want 0", s.Version())
	}

	for i := int32(0); i < 5; i++ {
		s.Emit(i)
	}

	if s.Version() != 5 {
		t.Errorf("Version() = %d, want 5", s.Version())
	}
}

func TestStreamBufferFull(t *testing.T) {
	name := fmt.Sprintf("/stream_bufferfull_%d", os.Getpid())
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	s, err := NewStream[int32](mem, "strm", 3)
	if err != nil {
		t.Fatalf("NewStream failed: %v", err)
	}

	// Fill the buffer
	for i := int32(0); i < 3; i++ {
		if !s.Emit(i) {
			t.Fatalf("Emit(%d) failed, should not be full yet", i)
		}
	}

	// Buffer is now full
	if s.Emit(99) {
		t.Error("Emit on full buffer should return false")
	}

	if s.Size() != 3 {
		t.Errorf("Size() = %d, want 3", s.Size())
	}
}

func TestStreamClose(t *testing.T) {
	name := fmt.Sprintf("/stream_close_%d", os.Getpid())
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	s, err := NewStream[int32](mem, "strm", 10)
	if err != nil {
		t.Fatalf("NewStream failed: %v", err)
	}

	s.Emit(1)
	s.Close()

	// Cannot emit after close
	if s.Emit(99) {
		t.Error("Emit after Close should return false")
	}

	// Can still drain existing items
	value, ok := s.Read()
	if !ok {
		t.Fatal("Read after Close should drain remaining items")
	}
	if value != 1 {
		t.Errorf("Read() = %d, want 1", value)
	}

	// No more items
	_, ok = s.Read()
	if ok {
		t.Error("Read on closed, empty stream should return false")
	}
}

func TestStreamIsClosed(t *testing.T) {
	name := fmt.Sprintf("/stream_isclosed_%d", os.Getpid())
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	s, err := NewStream[int32](mem, "strm", 10)
	if err != nil {
		t.Fatalf("NewStream failed: %v", err)
	}

	if s.IsClosed() {
		t.Error("IsClosed() should be false initially")
	}

	s.Close()

	if !s.IsClosed() {
		t.Error("IsClosed() should be true after Close()")
	}
}

func TestStreamReadTimeoutSuccess(t *testing.T) {
	name := fmt.Sprintf("/stream_readtimeout_ok_%d", os.Getpid())
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	s, err := NewStream[int32](mem, "strm", 10)
	if err != nil {
		t.Fatalf("NewStream failed: %v", err)
	}

	// Emit in another goroutine after a short delay
	go func() {
		time.Sleep(10 * time.Millisecond)
		s.Emit(77)
	}()

	value, ok := s.ReadTimeout(500 * time.Millisecond)
	if !ok {
		t.Fatal("ReadTimeout should succeed when data is available")
	}
	if value != 77 {
		t.Errorf("ReadTimeout() = %d, want 77", value)
	}
}

func TestStreamReadTimeoutFail(t *testing.T) {
	name := fmt.Sprintf("/stream_readtimeout_fail_%d", os.Getpid())
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	s, err := NewStream[int32](mem, "strm", 10)
	if err != nil {
		t.Fatalf("NewStream failed: %v", err)
	}

	_, ok := s.ReadTimeout(50 * time.Millisecond)
	if ok {
		t.Error("ReadTimeout on empty stream should return false")
	}
}

func TestStreamReadTimeoutClosed(t *testing.T) {
	name := fmt.Sprintf("/stream_readtimeout_closed_%d", os.Getpid())
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	s, err := NewStream[int32](mem, "strm", 10)
	if err != nil {
		t.Fatalf("NewStream failed: %v", err)
	}

	s.Close()

	_, ok := s.ReadTimeout(50 * time.Millisecond)
	if ok {
		t.Error("ReadTimeout on closed stream should return false")
	}
}

func TestStreamSubscribe(t *testing.T) {
	name := fmt.Sprintf("/stream_subscribe_%d", os.Getpid())
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	s, err := NewStream[int32](mem, "strm", 10)
	if err != nil {
		t.Fatalf("NewStream failed: %v", err)
	}

	if s.Subscribers() != 0 {
		t.Errorf("Subscribers() = %d, want 0", s.Subscribers())
	}

	unsub1 := s.Subscribe()
	if s.Subscribers() != 1 {
		t.Errorf("Subscribers() = %d, want 1", s.Subscribers())
	}

	unsub2 := s.Subscribe()
	if s.Subscribers() != 2 {
		t.Errorf("Subscribers() = %d, want 2", s.Subscribers())
	}

	unsub1()
	if s.Subscribers() != 1 {
		t.Errorf("Subscribers() = %d, want 1 after unsub1", s.Subscribers())
	}

	unsub2()
	if s.Subscribers() != 0 {
		t.Errorf("Subscribers() = %d, want 0 after unsub2", s.Subscribers())
	}
}

func TestStreamNewNameTooLong(t *testing.T) {
	name := fmt.Sprintf("/stream_toolong_%d", os.Getpid())
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	longName := make([]byte, 300)
	for i := range longName {
		longName[i] = 'a'
	}

	_, err = NewStream[int32](mem, string(longName), 10)
	if err == nil {
		t.Error("NewStream with name too long should fail")
	}
}

func TestStreamNewNameTooShort(t *testing.T) {
	name := fmt.Sprintf("/stream_tooshort_%d", os.Getpid())
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	_, err = NewStream[int32](mem, "st", 10)
	if err == nil {
		t.Error("NewStream with name too short should fail")
	}
}

func TestStreamNewAlreadyExists(t *testing.T) {
	name := fmt.Sprintf("/stream_exists_%d", os.Getpid())
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	_, err = NewStream[int32](mem, "strm", 10)
	if err != nil {
		t.Fatalf("First NewStream failed: %v", err)
	}

	_, err = NewStream[int32](mem, "strm", 10)
	if err == nil {
		t.Error("Second NewStream with same name should fail")
	}
}

func TestStreamOpenNotExists(t *testing.T) {
	name := fmt.Sprintf("/stream_opennotfound_%d", os.Getpid())
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	_, err = OpenStream[int32](mem, "nonexistent")
	if err == nil {
		t.Error("OpenStream with non-existent name should fail")
	}
}

func TestStreamOpenCrossProcess(t *testing.T) {
	name := fmt.Sprintf("/stream_cross_%d", os.Getpid())
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	s1, err := NewStream[int32](mem, "strm", 10)
	if err != nil {
		t.Fatalf("NewStream failed: %v", err)
	}

	// Emit some data
	s1.Emit(100)
	s1.Emit(200)

	// Open in a second handle (simulating cross-process)
	mem2, err := OpenMemory(name, 64)
	if err != nil {
		t.Fatalf("OpenMemory failed: %v", err)
	}
	defer mem2.Close()

	s2, err := OpenStream[int32](mem2, "strm")
	if err != nil {
		t.Fatalf("OpenStream failed: %v", err)
	}

	// Read the data emitted by the first handle
	value, ok := s2.Read()
	if !ok {
		t.Fatal("Read from second handle failed")
	}
	if value != 100 {
		t.Errorf("Read() = %d, want 100", value)
	}

	value, ok = s2.Read()
	if !ok {
		t.Fatal("Second Read from second handle failed")
	}
	if value != 200 {
		t.Errorf("Read() = %d, want 200", value)
	}
}

func TestStreamInt64(t *testing.T) {
	name := fmt.Sprintf("/stream_int64_%d", os.Getpid())
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	s, err := NewStream[int64](mem, "strm", 10)
	if err != nil {
		t.Fatalf("NewStream[int64] failed: %v", err)
	}

	if !s.Emit(int64(9223372036854775807)) {
		t.Fatal("Emit(int64 max) failed")
	}

	value, ok := s.Read()
	if !ok {
		t.Fatal("Read failed")
	}
	if value != int64(9223372036854775807) {
		t.Errorf("Read() = %d, want %d", value, int64(9223372036854775807))
	}
}

func TestStreamFloat64(t *testing.T) {
	name := fmt.Sprintf("/stream_float64_%d", os.Getpid())
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	s, err := NewStream[float64](mem, "strm", 10)
	if err != nil {
		t.Fatalf("NewStream[float64] failed: %v", err)
	}

	if !s.Emit(3.14159) {
		t.Fatal("Emit(float64) failed")
	}

	value, ok := s.Read()
	if !ok {
		t.Fatal("Read failed")
	}
	if value != 3.14159 {
		t.Errorf("Read() = %v, want 3.14159", value)
	}
}

func TestStreamDestroyAndReuse(t *testing.T) {
	name := fmt.Sprintf("/stream_destroy_%d", os.Getpid())
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	s, err := NewStream[int32](mem, "strm", 10)
	if err != nil {
		t.Fatalf("NewStream failed: %v", err)
	}

	s.Destroy()

	// Name should be reusable now
	s2, err := NewStream[int32](mem, "strm", 10)
	if err != nil {
		t.Fatalf("NewStream after Destroy failed: %v", err)
	}

	if s2.Name() != "strm" {
		t.Errorf("Name() = %q, want %q", s2.Name(), "strm")
	}
}

func TestStreamCloseThenDrain(t *testing.T) {
	name := fmt.Sprintf("/stream_closedrain_%d", os.Getpid())
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	s, err := NewStream[int32](mem, "strm", 10)
	if err != nil {
		t.Fatalf("NewStream failed: %v", err)
	}

	// Emit 5 items
	for i := int32(0); i < 5; i++ {
		s.Emit(i)
	}

	// Close the stream
	s.Close()

	// Drain all 5 items
	for i := int32(0); i < 5; i++ {
		value, ok := s.Read()
		if !ok {
			t.Fatalf("Drain at %d failed", i)
		}
		if value != i {
			t.Errorf("Drain at %d = %d, want %d", i, value, i)
		}
	}

	// After draining, Read should return false
	_, ok := s.Read()
	if ok {
		t.Error("Read on closed, drained stream should return false")
	}
}

func TestStreamConcurrentProducers(t *testing.T) {
	name := fmt.Sprintf("/stream_concurrent_%d", os.Getpid())
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()
	defer mem.Unlink()

	s, err := NewStream[int32](mem, "strm", 1000)
	if err != nil {
		t.Fatalf("NewStream failed: %v", err)
	}

	done := make(chan struct{})
	var emitted int

	// Multiple producers
	for p := int32(0); p < 4; p++ {
		go func(id int32) {
			for i := int32(0); i < 50; i++ {
				for !s.Emit(id*1000+i) {
					// Buffer full, wait and retry
					time.Sleep(time.Microsecond)
				}
			}
			done <- struct{}{}
		}(p)
	}

	// Consumer in main goroutine
	for range 200 {
		v, ok := s.ReadTimeout(2 * time.Second)
		if !ok {
			break
		}
		_ = v
		emitted++
	}

	// Wait for producers
	for range 4 {
		select {
		case <-done:
		case <-time.After(5 * time.Second):
			t.Fatal("Producer timed out")
		}
	}

	s.Close()

	// Drain remaining
	for {
		_, ok := s.Read()
		if !ok {
			break
		}
		emitted++
	}

	if emitted != 200 {
		t.Errorf("Emitted %d, want 200", emitted)
	}
}
