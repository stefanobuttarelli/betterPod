//go:build ignore

package main

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

func TestPrefsWriterCoalescesImmutableSnapshots(t *testing.T) {
	var writes atomic.Int32
	var gotPath string
	var gotData []byte
	w := newPrefsWriter(time.Hour, time.Hour, func(path string, data []byte) error {
		writes.Add(1)
		gotPath, gotData = path, append([]byte(nil), data...)
		return nil
	})
	defer w.close()
	for i := 0; i < 100; i++ {
		w.enqueue("settings.json", []byte{byte(i)})
	}
	last := []byte("latest")
	w.enqueue("settings.json", last)
	last[0] = 'X'
	if writes.Load() != 0 {
		t.Fatal("enqueue performed a write before the debounce or a barrier")
	}
	if err := w.flush(); err != nil {
		t.Fatal(err)
	}
	if writes.Load() != 1 || gotPath != "settings.json" || string(gotData) != "latest" {
		t.Fatalf("writes=%d path=%q data=%q", writes.Load(), gotPath, gotData)
	}
}

func TestPrefsWriterEnqueueDoesNotWaitForDisk(t *testing.T) {
	started, release := make(chan struct{}), make(chan struct{})
	var once sync.Once
	var writes atomic.Int32
	var latest []byte
	w := newPrefsWriter(0, 0, func(_ string, data []byte) error {
		if writes.Add(1) == 1 {
			close(started)
			<-release // simulate a slow fsync without blocking the UI
		}
		latest = append([]byte(nil), data...)
		return nil
	})
	defer w.close()
	defer once.Do(func() { close(release) })
	w.enqueue("settings.json", []byte("first"))
	select {
	case <-started:
	case <-time.After(time.Second):
		t.Fatal("writer did not start")
	}
	queued := make(chan struct{})
	go func() {
		for i := 0; i < 100; i++ {
			w.enqueue("settings.json", []byte{byte(i)})
		}
		close(queued)
	}()
	select {
	case <-queued:
	case <-time.After(time.Second):
		t.Fatal("enqueue waited for the in-flight disk write")
	}
	once.Do(func() { close(release) })
	if err := w.flush(); err != nil {
		t.Fatal(err)
	}
	if writes.Load() != 2 || len(latest) != 1 || latest[0] != 99 {
		t.Fatalf("in-flight and latest snapshots: writes=%d latest=%v", writes.Load(), latest)
	}
}

func TestPrefsWriterDebounceAndMaximumWait(t *testing.T) {
	for _, tc := range []struct {
		name              string
		debounce, maximum time.Duration
	}{
		{"quiet burst", 20 * time.Millisecond, time.Hour},
		{"maximum wait", time.Hour, 20 * time.Millisecond},
	} {
		t.Run(tc.name, func(t *testing.T) {
			written := make(chan struct{}, 1)
			w := newPrefsWriter(tc.debounce, tc.maximum, func(string, []byte) error {
				written <- struct{}{}
				return nil
			})
			defer w.close()
			w.enqueue("settings.json", []byte("saved"))
			select {
			case <-written:
			case <-time.After(time.Second):
				t.Fatal("pending snapshot was not written by its timer")
			}
		})
	}
}

func TestPrefsWriterCloseFlushesAndStops(t *testing.T) {
	var writes atomic.Int32
	w := newPrefsWriter(time.Hour, time.Hour, func(string, []byte) error {
		writes.Add(1)
		return nil
	})
	w.enqueue("settings.json", []byte("latest before exit"))
	if err := w.close(); err != nil {
		t.Fatal(err)
	}
	w.enqueue("settings.json", []byte("late request after shutdown"))
	if err := w.close(); err != nil {
		t.Fatal(err)
	}
	if writes.Load() != 1 {
		t.Fatalf("close wrote %d snapshots", writes.Load())
	}
	select {
	case <-w.done:
	default:
		t.Fatal("writer remained running after close")
	}
}

func TestPrefsWriterFlushReportsFailureAndCanSaveAgain(t *testing.T) {
	failure := errors.New("disk unavailable")
	w := newPrefsWriter(time.Hour, time.Hour, func(_ string, data []byte) error {
		if string(data) == "bad" {
			return failure
		}
		return nil
	})
	defer w.close()
	w.enqueue("settings.json", []byte("bad"))
	if err := w.flush(); !errors.Is(err, failure) {
		t.Fatalf("flush lost the write error: %v", err)
	}
	w.enqueue("settings.json", []byte("good"))
	if err := w.flush(); err != nil {
		t.Fatalf("later snapshot could not be saved: %v", err)
	}
}

func prefsTestWriter(t *testing.T, dir string) {
	t.Helper()
	if err := flushPrefs(); err != nil {
		t.Fatal(err)
	}
	oldWriter := preferenceWriter
	dataDirMu.Lock()
	oldDir := dataDirOverride
	dataDirOverride = dir
	dataDirMu.Unlock()
	preferenceWriter = newPrefsWriter(time.Hour, time.Hour, persistPrefs)
	t.Cleanup(func() {
		if err := closePrefs(); err != nil {
			t.Error(err)
		}
		preferenceWriter = oldWriter
		dataDirMu.Lock()
		dataDirOverride = oldDir
		dataDirMu.Unlock()
	})
}

func TestPrefsSaveDoesNotTouchDiskPerKey(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "not-created-until-write")
	prefsTestWriter(t, dir)
	ui := &IpodUI{app: &BetterPod{Queue: &PlaybackQueue{Repeat: RepeatAll}}}
	ui.volume = .25
	ui.savePrefs()
	ui.volume = .75
	ui.savePrefs()
	if _, err := os.Stat(dir); !os.IsNotExist(err) {
		t.Fatalf("savePrefs touched the filesystem before its worker/barrier: %v", err)
	}
	if err := flushPrefs(); err != nil {
		t.Fatal(err)
	}
	data, err := os.ReadFile(filepath.Join(dir, ipodPrefsFile))
	if err != nil {
		t.Fatal(err)
	}
	var got ipodPrefs
	if err := json.Unmarshal(data, &got); err != nil || got.Volume != .75 || got.RepeatMode != "all" {
		t.Fatalf("latest preference schema was not preserved: %s (%v)", data, err)
	}
}

func TestPrefsDatabaseSwitchDrainsOldPath(t *testing.T) {
	base := t.TempDir()
	old, next := filepath.Join(base, "old"), filepath.Join(base, "next")
	t.Setenv("BETTERPOD_DATA_DIR", base)
	prefsTestWriter(t, old)
	queuePrefs([]byte("latest old snapshot"))
	if err := os.MkdirAll(next, 0755); err != nil {
		t.Fatal(err)
	}
	if err := switchDataDirWithPrefs(old, next); err != nil {
		t.Fatal(err)
	}
	read := func(dir, want string) {
		t.Helper()
		got, err := os.ReadFile(filepath.Join(dir, ipodPrefsFile))
		if err != nil || string(got) != want {
			t.Fatalf("%s: got %q, error %v, want %q", dir, got, err, want)
		}
	}
	read(next, "latest old snapshot")
	if err := os.RemoveAll(old); err != nil {
		t.Fatal(err)
	}
	queuePrefs([]byte("new folder snapshot"))
	if err := flushPrefs(); err != nil {
		t.Fatal(err)
	}
	read(next, "new folder snapshot")
	if _, err := os.Stat(old); !os.IsNotExist(err) {
		t.Fatalf("a stale writer recreated the old database: %v", err)
	}
}

func TestPrefsUseExistingDatabaseKeepsItsPreferences(t *testing.T) {
	base := t.TempDir()
	old, next := filepath.Join(base, "old"), filepath.Join(base, "existing")
	t.Setenv("BETTERPOD_DATA_DIR", base)
	prefsTestWriter(t, old)
	if err := os.MkdirAll(next, 0755); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(next, ipodPrefsFile)
	if err := os.WriteFile(path, []byte("existing database preferences"), 0644); err != nil {
		t.Fatal(err)
	}
	queuePrefs([]byte("old database preferences"))
	if err := setDataDir(next); err != nil {
		t.Fatal(err)
	}
	got, err := os.ReadFile(path)
	if err != nil || string(got) != "existing database preferences" {
		t.Fatalf("switch overwrote the selected database's preferences: %q, %v", got, err)
	}
	got, err = os.ReadFile(filepath.Join(old, ipodPrefsFile))
	if err != nil || string(got) != "old database preferences" {
		t.Fatalf("switch did not drain the previous database's save: %q, %v", got, err)
	}
}

func TestPrefsMoveFinalCopyKeepsNewDestinationSettings(t *testing.T) {
	from, to := t.TempDir(), t.TempDir()
	for path, contents := range map[string]string{
		filepath.Join(from, ipodPrefsFile): "preferences before switch",
		filepath.Join(to, ipodPrefsFile):   "preferences changed after switch",
		filepath.Join(from, libraryFile):   "latest library",
	} {
		if err := os.WriteFile(path, []byte(contents), 0644); err != nil {
			t.Fatal(err)
		}
	}
	if err := copyDatabaseAfterSwitch(from, to); err != nil {
		t.Fatal(err)
	}
	got, err := os.ReadFile(filepath.Join(to, ipodPrefsFile))
	if err != nil || string(got) != "preferences changed after switch" {
		t.Fatalf("the move's final pass overwrote new preferences: %q, %v", got, err)
	}
	got, err = os.ReadFile(filepath.Join(to, libraryFile))
	if err != nil || string(got) != "latest library" {
		t.Fatalf("the move's final pass missed the remaining database: %q, %v", got, err)
	}
}

func TestPrefsWriterRetriesOnlyAtExplicitBarrier(t *testing.T) {
	failure := errors.New("card temporarily missing")
	var attempts atomic.Int32
	w := newPrefsWriter(time.Hour, time.Hour, func(_ string, data []byte) error {
		if string(data) != "latest settings" {
			t.Error("retry changed the immutable snapshot")
		}
		if attempts.Add(1) == 1 {
			return failure
		}
		return nil
	})
	defer w.close()
	w.enqueue("settings.json", []byte("latest settings"))
	if err := w.flush(); !errors.Is(err, failure) {
		t.Fatalf("first attempt: %v", err)
	}
	time.Sleep(30 * time.Millisecond)
	if attempts.Load() != 1 {
		t.Fatalf("failed writer retried without a barrier: %d", attempts.Load())
	}
	if err := w.flush(); err != nil {
		t.Fatalf("restored card was not retried: %v", err)
	}
	if attempts.Load() != 2 {
		t.Fatalf("explicit retry attempts=%d", attempts.Load())
	}
	if err := w.flush(); err != nil || attempts.Load() != 2 {
		t.Fatalf("successful retry retained an old error or repeated the write: %v, %d", err, attempts.Load())
	}
}

func TestPrefsWriterCloseRetriesPriorFailure(t *testing.T) {
	failure := errors.New("temporary write failure")
	var attempts atomic.Int32
	w := newPrefsWriter(time.Hour, time.Hour, func(string, []byte) error {
		if attempts.Add(1) == 1 {
			return failure
		}
		return nil
	})
	w.enqueue("settings.json", []byte("latest"))
	if err := w.flush(); !errors.Is(err, failure) {
		t.Fatalf("first attempt: %v", err)
	}
	if err := w.close(); err != nil || attempts.Load() != 2 {
		t.Fatalf("close did not retry the last unsaved snapshot: %v, %d", err, attempts.Load())
	}
}

func TestPrefsWriterCloseReturnsOnlyCurrentAttemptError(t *testing.T) {
	first, current := errors.New("card missing"), errors.New("card read-only")
	var attempts atomic.Int32
	w := newPrefsWriter(time.Hour, time.Hour, func(string, []byte) error {
		if attempts.Add(1) == 1 {
			return first
		}
		return current
	})
	w.enqueue("settings.json", []byte("latest"))
	if err := w.flush(); !errors.Is(err, first) {
		t.Fatalf("first attempt: %v", err)
	}
	closed := make(chan error, 1)
	go func() { closed <- w.close() }()
	select {
	case err := <-closed:
		if !errors.Is(err, current) || errors.Is(err, first) {
			t.Fatalf("close returned a historical error instead of its attempt: %v", err)
		}
	case <-time.After(time.Second):
		t.Fatal("failed close did not stop after its current attempt")
	}
	if attempts.Load() != 2 {
		t.Fatalf("failed close retried automatically: %d attempts", attempts.Load())
	}
	select {
	case <-w.done:
	default:
		t.Fatal("failed close left its worker running")
	}
}

func TestPrefsWriterFailedCloseCanRetryWithoutReopeningSaves(t *testing.T) {
	failure := errors.New("temporary write failure")
	var attempts atomic.Int32
	w := newPrefsWriter(time.Hour, time.Hour, func(_ string, data []byte) error {
		if string(data) != "latest before close" {
			t.Error("shutdown accepted a new UI snapshot")
		}
		if attempts.Add(1) == 1 {
			return failure
		}
		return nil
	})
	w.enqueue("settings.json", []byte("latest before close"))
	if err := w.close(); !errors.Is(err, failure) {
		t.Fatalf("failed close: %v", err)
	}
	w.enqueue("settings.json", []byte("late UI request"))
	if err := w.close(); err != nil || attempts.Load() != 2 {
		t.Fatalf("later close did not retry exactly the previous snapshot: %v, %d", err, attempts.Load())
	}
	if err := w.close(); err != nil || attempts.Load() != 2 {
		t.Fatalf("completed shutdown wrote again: %v, %d", err, attempts.Load())
	}
}

func TestPrefsWriterConcurrentRetryBarriersShareAttempt(t *testing.T) {
	failure := errors.New("temporary write failure")
	entered, release := make(chan struct{}), make(chan struct{})
	var unblock sync.Once
	var attempts atomic.Int32
	w := newPrefsWriter(time.Hour, time.Hour, func(string, []byte) error {
		if attempts.Add(1) == 1 {
			return failure
		}
		close(entered)
		<-release
		return nil
	})
	defer w.close()
	defer unblock.Do(func() { close(release) })
	w.enqueue("settings.json", []byte("latest"))
	if err := w.flush(); !errors.Is(err, failure) {
		t.Fatal(err)
	}
	results := make(chan error, 2)
	go func() { results <- w.flush() }()
	select {
	case <-entered:
	case <-time.After(time.Second):
		t.Fatal("retry did not start")
	}
	go func() { results <- w.close() }()
	deadline := time.Now().Add(time.Second)
	for {
		w.mu.Lock()
		waiting := len(w.waiters)
		w.mu.Unlock()
		if waiting == 2 {
			break
		}
		if time.Now().After(deadline) {
			t.Fatal("concurrent barriers did not join the in-flight retry")
		}
		time.Sleep(time.Millisecond)
	}
	unblock.Do(func() { close(release) })
	for i := 0; i < 2; i++ {
		select {
		case err := <-results:
			if err != nil {
				t.Fatal(err)
			}
		case <-time.After(time.Second):
			t.Fatal("retry barrier deadlocked")
		}
	}
	if attempts.Load() != 2 {
		t.Fatalf("concurrent barriers duplicated the retry: %d attempts", attempts.Load())
	}
}
