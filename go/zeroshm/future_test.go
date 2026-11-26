package zeroshm

import (
	"fmt"
	"os"
	"strings"
	"sync"
	"testing"
	"time"
)

func setupFutureTest[T Numeric](t *testing.T, namePrefix string) (*Memory, *Future[T]) {
	t.Helper()
	name := fmt.Sprintf("/future_%s_%d", namePrefix, os.Getpid())
	UnlinkName(name)
	t.Cleanup(func() { UnlinkName(name) })

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	t.Cleanup(func() { mem.Close() })

	future, err := NewFuture[T](mem, "future_"+namePrefix)
	if err != nil {
		t.Fatalf("NewFuture failed: %v", err)
	}
	t.Cleanup(func() { future.Destroy() })

	return mem, future
}

func TestFutureName(t *testing.T) {
	_, future := setupFutureTest[int32](t, "name")
	if future.Name() != "future_name" {
		t.Errorf("expected name 'future_name', got %q", future.Name())
	}
}

func TestFutureInitialState(t *testing.T) {
	_, future := setupFutureTest[int32](t, "initial")
	if future.State() != FutureStatePending {
		t.Errorf("expected PENDING state, got %d", future.State())
	}
	if future.IsReady() {
		t.Error("expected future to not be ready")
	}
}

func TestFutureSetValueGet(t *testing.T) {
	_, future := setupFutureTest[int32](t, "setget")

	if !future.SetValue(42) {
		t.Fatal("SetValue failed")
	}
	if future.State() != FutureStateReady {
		t.Errorf("expected READY state, got %d", future.State())
	}
	if !future.IsReady() {
		t.Error("expected future to be ready")
	}

	val, err := future.Get()
	if err != nil {
		t.Fatalf("Get failed: %v", err)
	}
	if val != 42 {
		t.Errorf("expected value 42, got %d", val)
	}
}

func TestFutureTryGetNotReady(t *testing.T) {
	_, future := setupFutureTest[int32](t, "trynotready")

	_, err := future.TryGet()
	if err != FutureNotReady {
		t.Errorf("expected FutureNotReady, got %v", err)
	}
}

func TestFutureTryGetReady(t *testing.T) {
	_, future := setupFutureTest[int32](t, "tryready")

	future.SetValue(99)
	val, err := future.TryGet()
	if err != nil {
		t.Fatalf("TryGet failed: %v", err)
	}
	if val != 99 {
		t.Errorf("expected 99, got %d", val)
	}
}

func TestFutureSetValueOnce(t *testing.T) {
	_, future := setupFutureTest[int32](t, "setonce")

	if !future.SetValue(42) {
		t.Fatal("First SetValue failed")
	}
	if future.SetValue(99) {
		t.Error("expected second SetValue to fail")
	}

	val, err := future.Get()
	if err != nil {
		t.Fatalf("Get failed: %v", err)
	}
	if val != 42 {
		t.Errorf("expected 42 (first value), got %d", val)
	}
}

func TestFutureSetError(t *testing.T) {
	_, future := setupFutureTest[int32](t, "seterror")

	if !future.SetError("something went wrong") {
		t.Fatal("SetError failed")
	}
	if future.State() != FutureStateError {
		t.Errorf("expected ERROR state, got %d", future.State())
	}
	if !future.IsReady() {
		t.Error("expected future to be ready")
	}

	_, err := future.Get()
	if err == nil {
		t.Fatal("expected error from Get")
	}
	if !strings.Contains(err.Error(), "something went wrong") {
		t.Errorf("expected error message to contain 'something went wrong', got %q", err.Error())
	}
}

func TestFutureGetTimeoutSuccess(t *testing.T) {
	_, future := setupFutureTest[int32](t, "timeoutsuccess")

	// Set value in a goroutine after a short delay
	go func() {
		time.Sleep(20 * time.Millisecond)
		future.SetValue(77)
	}()

	val, ok, err := future.GetTimeout(100 * time.Millisecond)
	if err != nil {
		t.Fatalf("GetTimeout failed: %v", err)
	}
	if !ok {
		t.Error("expected value within timeout")
	}
	if val != 77 {
		t.Errorf("expected 77, got %d", val)
	}
}

func TestFutureGetTimeoutFail(t *testing.T) {
	_, future := setupFutureTest[int32](t, "timeoutfail")

	val, ok, err := future.GetTimeout(50 * time.Millisecond)
	if err != nil {
		t.Fatalf("GetTimeout failed: %v", err)
	}
	if ok {
		t.Error("expected timeout, got value")
	}
	if val != 0 {
		t.Errorf("expected zero value on timeout, got %d", val)
	}
}

func TestFutureGetBlocks(t *testing.T) {
	_, future := setupFutureTest[int32](t, "blocks")

	var wg sync.WaitGroup
	var received int32

	wg.Add(1)
	go func() {
		defer wg.Done()
		val, err := future.Get()
		if err != nil {
			t.Errorf("Get failed: %v", err)
			return
		}
		received = val
	}()

	// Give goroutine time to start waiting
	time.Sleep(10 * time.Millisecond)

	// Set value -- should unblock the waiter
	future.SetValue(55)

	wg.Wait()

	if received != 55 {
		t.Errorf("expected 55, got %d", received)
	}
}

func TestFutureOpenCrossProcess(t *testing.T) {
	_, future := setupFutureTest[int32](t, "crossproc")

	future.SetValue(123)

	future2, err := OpenFuture[int32](future.memory, "future_crossproc")
	if err != nil {
		t.Fatalf("OpenFuture failed: %v", err)
	}
	defer future2.Destroy()

	val, err := future2.Get()
	if err != nil {
		t.Fatalf("Get from opened future failed: %v", err)
	}
	if val != 123 {
		t.Errorf("expected 123, got %d", val)
	}
}

func TestFutureOpenNotFound(t *testing.T) {
	name := fmt.Sprintf("/future_notfound_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()

	_, err = OpenFuture[int32](mem, "nonexistent")
	if err == nil {
		t.Error("expected error for non-existent future")
	}
}

func TestFutureNewNameTooLong(t *testing.T) {
	name := fmt.Sprintf("/future_longname_%d", os.Getpid())
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
	_, err = NewFuture[int32](mem, longName)
	if err == nil {
		t.Error("expected error for name too long")
	}
}

func TestFutureNewNameTooShort(t *testing.T) {
	name := fmt.Sprintf("/future_shortname_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()

	_, err = NewFuture[int32](mem, "ab")
	if err == nil {
		t.Error("expected error for name too short")
	}
}

func TestFutureNewAlreadyExists(t *testing.T) {
	_, future := setupFutureTest[int32](t, "dup")
	_ = future

	_, err := NewFuture[int32](future.memory, "future_dup")
	if err == nil {
		t.Error("expected error for duplicate name")
	}
}

func TestFutureOpenTypeMismatch(t *testing.T) {
	_, future := setupFutureTest[int32](t, "typemismatch")
	_ = future

	_, err := OpenFuture[int64](future.memory, "future_typemismatch")
	if err == nil {
		t.Error("expected error for type mismatch")
	}
}

func TestFutureMultipleWaiters(t *testing.T) {
	_, future := setupFutureTest[int32](t, "multiwaiters")

	const numWaiters = 4
	results := make([]int32, numWaiters)
	var wg sync.WaitGroup

	for i := 0; i < numWaiters; i++ {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			val, err := future.Get()
			if err != nil {
				t.Errorf("Waiter %d Get failed: %v", i, err)
				return
			}
			results[i] = val
		}(i)
	}

	// Give waiters time to start
	time.Sleep(10 * time.Millisecond)

	future.SetValue(88)

	wg.Wait()

	for i, val := range results {
		if val != 88 {
			t.Errorf("waiter %d: expected 88, got %d", i, val)
		}
	}
}

func TestFutureInt64(t *testing.T) {
	_, future := setupFutureTest[int64](t, "int64")

	future.SetValue(int64(1234567890123))
	val, err := future.Get()
	if err != nil {
		t.Fatalf("Get failed: %v", err)
	}
	if val != int64(1234567890123) {
		t.Errorf("expected 1234567890123, got %d", val)
	}
}

func TestFutureFloat64(t *testing.T) {
	_, future := setupFutureTest[float64](t, "float64")

	future.SetValue(3.14159)
	val, err := future.Get()
	if err != nil {
		t.Fatalf("Get failed: %v", err)
	}
	if val < 3.14158 || val > 3.14160 {
		t.Errorf("expected ~3.14159, got %f", val)
	}
}

func TestFutureDestroyAndReuse(t *testing.T) {
	name := fmt.Sprintf("/future_destroy_%d", os.Getpid())
	UnlinkName(name)
	defer UnlinkName(name)

	mem, err := NewMemory(name, 1024*1024, 64)
	if err != nil {
		t.Fatalf("NewMemory failed: %v", err)
	}
	defer mem.Close()

	future, err := NewFuture[int32](mem, "future_destroy")
	if err != nil {
		t.Fatalf("NewFuture failed: %v", err)
	}
	future.Destroy()

	// Should be able to create again
	future2, err := NewFuture[int32](mem, "future_destroy")
	if err != nil {
		t.Fatalf("NewFuture after destroy failed: %v", err)
	}
	future2.Destroy()
}
