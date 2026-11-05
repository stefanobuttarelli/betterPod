package main

// Preference snapshots are made on the UI goroutine. A single lazy worker
// coalesces a burst of changes and writes only its latest immutable snapshot.
// Its timer exists only while a save is pending; idle playback adds no wakeups.

import (
	"fmt"
	"os"
	"path/filepath"
	"sync"
	"time"
)

type prefsSnapshot struct {
	path string
	data []byte
	seq  uint64
}

type prefsWaiter struct {
	seq  uint64
	done chan error
}

type prefsWriter struct {
	mu                sync.Mutex
	debounce, maxWait time.Duration
	write             func(string, []byte) error
	pending           *prefsSnapshot
	failed            *prefsSnapshot // retry only on a later explicit barrier
	first, last       time.Time
	seq, completed    uint64
	lastErr           error
	waiters           []prefsWaiter
	started, closed   bool
	wake              chan struct{}
	done              chan struct{}
}

func newPrefsWriter(debounce, maxWait time.Duration, write func(string, []byte) error) *prefsWriter {
	return &prefsWriter{debounce: debounce, maxWait: maxWait, write: write,
		wake: make(chan struct{}, 1), done: make(chan struct{})}
}

func (w *prefsWriter) signal() {
	select {
	case w.wake <- struct{}{}:
	default:
	}
}

// enqueue owns a copy: the worker never reads mutable UI state or a caller's
// reused buffer. It does not wait for the current write or perform any I/O.
func (w *prefsWriter) enqueue(path string, data []byte) {
	w.mu.Lock()
	if w.closed {
		w.mu.Unlock()
		return
	}
	now := time.Now()
	if w.pending == nil {
		w.first = now
	}
	w.last = now
	w.seq++
	w.pending = &prefsSnapshot{path: path, data: append([]byte(nil), data...), seq: w.seq}
	w.failed = nil // an older failed snapshot must never replace this one
	w.startLocked()
	w.mu.Unlock()
	w.signal()
}

func (w *prefsWriter) startLocked() {
	if !w.started {
		w.started = true
		w.done = make(chan struct{})
		go w.run()
	}
}

// retryLocked schedules at most one retry for this barrier. Failed writes
// remain idle until a later flush/close, so a missing card cannot cause a
// repeated write loop. A closed worker may restart for this explicit retry;
// ordinary snapshots are still rejected after shutdown starts.
func (w *prefsWriter) retryLocked() {
	if w.failed == nil || w.pending != nil || w.completed != w.seq {
		return
	}
	w.seq++
	snapshot := *w.failed
	snapshot.seq = w.seq
	w.pending, w.failed = &snapshot, nil
	w.first, w.last = time.Now(), time.Now()
	w.startLocked()
}

// waitLocked adds a barrier for everything queued so far. A coalesced newer
// snapshot satisfies earlier barriers because it contains the latest settings.
func (w *prefsWriter) waitLocked() (<-chan error, error) {
	if w.completed == w.seq {
		return nil, w.lastErr
	}
	done := make(chan error, 1)
	w.waiters = append(w.waiters, prefsWaiter{seq: w.seq, done: done})
	return done, nil
}

func (w *prefsWriter) flush() error {
	w.mu.Lock()
	w.retryLocked()
	done, err := w.waitLocked()
	w.mu.Unlock()
	w.signal()
	if done != nil {
		err = <-done
	}
	return err
}

func (w *prefsWriter) close() error {
	w.mu.Lock()
	w.closed = true
	w.retryLocked()
	if !w.started {
		select {
		case <-w.done:
		default:
			close(w.done)
		}
	}
	stopped := w.done
	done, err := w.waitLocked()
	w.mu.Unlock()
	w.signal()
	if done != nil {
		err = <-done
	}
	<-stopped
	return err
}

func (w *prefsWriter) run() {
	var timer *time.Timer
	defer func() {
		if timer != nil {
			timer.Stop()
		}
	}()
	for {
		w.mu.Lock()
		if w.pending == nil {
			if w.closed {
				w.started = false
				close(w.done)
				w.mu.Unlock()
				return
			}
			w.mu.Unlock()
			<-w.wake
			continue
		}
		deadline := w.last.Add(w.debounce)
		if limit := w.first.Add(w.maxWait); limit.Before(deadline) {
			deadline = limit
		}
		delay := time.Until(deadline)
		if w.closed || len(w.waiters) > 0 {
			delay = 0
		}
		if delay > 0 {
			w.mu.Unlock()
			if timer == nil {
				timer = time.NewTimer(delay)
			} else {
				timer.Reset(delay)
			}
			select {
			case <-timer.C:
			case <-w.wake:
				if !timer.Stop() {
					select {
					case <-timer.C:
					default:
					}
				}
			}
			continue
		}
		snapshot := w.pending
		w.pending = nil
		w.mu.Unlock()

		err := w.write(snapshot.path, snapshot.data)
		w.mu.Lock()
		w.completed, w.lastErr = snapshot.seq, err
		if err != nil && snapshot.seq == w.seq {
			w.failed = snapshot
		} else {
			w.failed = nil
		}
		keep := w.waiters[:0]
		for _, waiter := range w.waiters {
			if waiter.seq <= w.completed {
				waiter.done <- err
			} else {
				keep = append(keep, waiter)
			}
		}
		w.waiters = keep
		w.mu.Unlock()
	}
}

// Serialize snapshots and directory transitions. A transition drains old-path
// saves before changing the directory, so none can recreate a moved database.
var prefsSaveMu sync.Mutex
var preferenceWriter = newPrefsWriter(250*time.Millisecond, 2*time.Second, persistPrefs)

func persistPrefs(path string, data []byte) error {
	err := os.MkdirAll(filepath.Dir(path), 0755)
	if err == nil {
		err = writeFileAtomic(path, data)
	}
	if err != nil {
		logMsg(fmt.Sprintf("Preferences: could not save %s: %v", path, err))
	}
	return err
}

func queuePrefs(data []byte) {
	prefsSaveMu.Lock()
	preferenceWriter.enqueue(ipodPrefsPath(), data)
	prefsSaveMu.Unlock()
}

// flushPrefs is a durability barrier; ordinary saves remain usable afterwards.
func flushPrefs() error {
	prefsSaveMu.Lock()
	defer prefsSaveMu.Unlock()
	return preferenceWriter.flush()
}

// closePrefs is idempotent. After it starts, later snapshots are ignored: the
// app is exiting or replacing its process, and the last queued state is saved.
func closePrefs() error {
	prefsSaveMu.Lock()
	defer prefsSaveMu.Unlock()
	return preferenceWriter.close()
}

func withPrefsPathChange(change func() error) error {
	prefsSaveMu.Lock()
	defer prefsSaveMu.Unlock()
	if err := preferenceWriter.flush(); err != nil {
		return fmt.Errorf("could not save preferences before changing folders: %w", err)
	}
	return change()
}
