package zeroshm

/*
#cgo CFLAGS: -pthread
#cgo LDFLAGS: -lpthread

#include <pthread.h>
#include <stddef.h>
#include <time.h>

// Return the size of pthread_mutex_t
static size_t zeroshm_pthread_mutex_size(void) {
    return sizeof(pthread_mutex_t);
}

// Return the alignment of pthread_mutex_t
static size_t zeroshm_pthread_mutex_align(void) {
    return __alignof__(pthread_mutex_t);
}

// Return the size of pthread_cond_t
static size_t zeroshm_pthread_cond_size(void) {
    return sizeof(pthread_cond_t);
}

// Return the alignment of pthread_cond_t
static size_t zeroshm_pthread_cond_align(void) {
    return __alignof__(pthread_cond_t);
}

// Initialize a mutex with PTHREAD_PROCESS_SHARED.
// The mutex must point to valid (zeroed) memory in shared memory.
static int zeroshm_pthread_mutex_init(pthread_mutex_t* mutex) {
    pthread_mutexattr_t attr;
    int ret;

    ret = pthread_mutexattr_init(&attr);
    if (ret != 0) return ret;

    ret = pthread_mutexattr_setpshared(&attr, PTHREAD_PROCESS_SHARED);
    if (ret != 0) {
        pthread_mutexattr_destroy(&attr);
        return ret;
    }

    ret = pthread_mutex_init(mutex, &attr);
    pthread_mutexattr_destroy(&attr);
    return ret;
}

// Initialize a condvar with PTHREAD_PROCESS_SHARED.
// The condvar must point to valid (zeroed) memory in shared memory.
static int zeroshm_pthread_cond_init(pthread_cond_t* cond) {
    pthread_condattr_t attr;
    int ret;

    ret = pthread_condattr_init(&attr);
    if (ret != 0) return ret;

    ret = pthread_condattr_setpshared(&attr, PTHREAD_PROCESS_SHARED);
    if (ret != 0) {
        pthread_condattr_destroy(&attr);
        return ret;
    }

    ret = pthread_cond_init(cond, &attr);
    pthread_condattr_destroy(&attr);
    return ret;
}

// Destroy a mutex. Only call from the process that owns the Latest.
static int zeroshm_pthread_mutex_destroy(pthread_mutex_t* mutex) {
    return pthread_mutex_destroy(mutex);
}

// Destroy a condvar. Only call from the process that owns the Latest.
static int zeroshm_pthread_cond_destroy(pthread_cond_t* cond) {
    return pthread_cond_destroy(cond);
}
*/
import "C"
import "unsafe"

// Pthread structure sizes and alignments for the unified Latest[T] layout.
// These are determined at package initialization from the platform's pthread.h.
var (
	pthreadMutexSize = int(C.zeroshm_pthread_mutex_size())
	pthreadMutexAlign = int(C.zeroshm_pthread_mutex_align())
	pthreadCondSize = int(C.zeroshm_pthread_cond_size())
	pthreadCondAlign = int(C.zeroshm_pthread_cond_align())
)

// initProcessSharedMutex initializes the mutex at the given shared memory pointer
// with PTHREAD_PROCESS_SHARED so it can be used across processes.
func initProcessSharedMutex(ptr unsafe.Pointer) {
	ret := C.zeroshm_pthread_mutex_init((*C.pthread_mutex_t)(ptr))
	if ret != 0 {
		panic("pthread_mutex_init failed")
	}
}

// initProcessSharedCond initializes the condvar at the given shared memory pointer
// with PTHREAD_PROCESS_SHARED so it can be used across processes.
func initProcessSharedCond(ptr unsafe.Pointer) {
	ret := C.zeroshm_pthread_cond_init((*C.pthread_cond_t)(ptr))
	if ret != 0 {
		panic("pthread_cond_init failed")
	}
}

// destroyMutex destroys the mutex at the given shared memory pointer.
func destroyMutex(ptr unsafe.Pointer) {
	C.zeroshm_pthread_mutex_destroy((*C.pthread_mutex_t)(ptr))
}

// destroyCond destroys the condvar at the given shared memory pointer.
func destroyCond(ptr unsafe.Pointer) {
	C.zeroshm_pthread_cond_destroy((*C.pthread_cond_t)(ptr))
}