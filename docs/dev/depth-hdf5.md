# HDF5 threading (depth)

**Never enrich `depth` from more than one thread.** HDF5 is commonly built
without thread safety, and such a build cannot be entered from several threads
*at all*. Mutual exclusion is not sufficient: locking so the calls never overlap
still crashes, because the library keeps state that assumes a single thread of
execution. This is why `DepthEnricher` returns `parallel() -> false`.

It cost a user a hard crash to find (a SIGSEGV on a 397 point input, where a 3
point input got through), so treat it as settled rather than something to
re-litigate.

- The `netcdf` crate already takes an exclusive process-wide lock around every
  netcdf-c call, so ordinary reads never overlap on their own. The mutex in
  `DepthEnricher` is belt and braces, kept because `silence_hdf5_diagnostics`
  calls `H5Eset_auto2` straight into HDF5, past that lock. Keep that call under
  the mutex.

- It only reproduces against a serial HDF5. A distribution `libhdf5-dev` is
  usually built thread-safe (Ubuntu's is), so the bug is invisible there and CI
  cannot catch it. The prebuilt release binaries are the vulnerable ones, since
  `static-netcdf` vendors HDF5 through cmake with thread safety off. Check any
  change to this area with:

  ```bash
  cargo test --features static-netcdf --test depth
  ```

- All the depth cases live in one `#[test]` on purpose. The harness gives each
  `#[test]` its own thread, which is by itself enough to trip a serial HDF5, so
  splitting them makes the suite abort at random even when the library is sound.
