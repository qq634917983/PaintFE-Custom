// Parallelism compatibility shim.
//
// `rayon` compiles fine for wasm32-unknown-unknown but panics at runtime the
// moment anything actually tries to synchronize with a worker thread
// (`Condvar::wait` has no implementation without real OS threads in the
// browser sandbox) — every `.par_iter()` etc. call would crash the app on
// every frame. On native, this module is a transparent passthrough to real
// rayon (unchanged parallel behavior). On wasm32, it provides the exact same
// method names running serially on the calling thread, so call sites don't
// need per-target branches.
//
// A real parallel web build would need `wasm-bindgen-rayon` (Web Workers +
// SharedArrayBuffer, which requires cross-origin-isolation headers) — out of
// scope for this port; see the compromises list.

#[cfg(not(target_arch = "wasm32"))]
pub use rayon::prelude::*;

#[cfg(not(target_arch = "wasm32"))]
pub fn spawn<F: FnOnce() + Send + 'static>(f: F) {
    rayon::spawn(f);
}

#[cfg(target_arch = "wasm32")]
pub fn spawn<F: FnOnce() + Send + 'static>(f: F) {
    // No threads on wasm32 — run synchronously in place instead of
    // backgrounding. Callers already poll a result channel each frame, so
    // this just means the result is available immediately.
    f();
}

#[cfg(target_arch = "wasm32")]
pub trait IntoParIterCompat: IntoIterator + Sized {
    fn into_par_iter(self) -> <Self as IntoIterator>::IntoIter {
        self.into_iter()
    }
}
#[cfg(target_arch = "wasm32")]
impl<T: IntoIterator> IntoParIterCompat for T {}

#[cfg(target_arch = "wasm32")]
pub trait ParIterCompat<T> {
    fn par_iter(&self) -> std::slice::Iter<'_, T>;
    fn par_iter_mut(&mut self) -> std::slice::IterMut<'_, T>;
}
#[cfg(target_arch = "wasm32")]
impl<T> ParIterCompat<T> for [T] {
    fn par_iter(&self) -> std::slice::Iter<'_, T> {
        self.iter()
    }
    fn par_iter_mut(&mut self) -> std::slice::IterMut<'_, T> {
        self.iter_mut()
    }
}

#[cfg(target_arch = "wasm32")]
pub trait ParChunksCompat<T> {
    fn par_chunks_mut(&mut self, size: usize) -> std::slice::ChunksMut<'_, T>;
    fn par_chunks_exact(&self, size: usize) -> std::slice::ChunksExact<'_, T>;
    fn par_chunks_exact_mut(&mut self, size: usize) -> std::slice::ChunksExactMut<'_, T>;
}
#[cfg(target_arch = "wasm32")]
impl<T> ParChunksCompat<T> for [T] {
    fn par_chunks_mut(&mut self, size: usize) -> std::slice::ChunksMut<'_, T> {
        self.chunks_mut(size)
    }
    fn par_chunks_exact(&self, size: usize) -> std::slice::ChunksExact<'_, T> {
        self.chunks_exact(size)
    }
    fn par_chunks_exact_mut(&mut self, size: usize) -> std::slice::ChunksExactMut<'_, T> {
        self.chunks_exact_mut(size)
    }
}
