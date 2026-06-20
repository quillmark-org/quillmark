using System;

namespace Quillmark;

/// <summary>
/// Base for the wrappers that own a native handle. Implements the standard
/// dispose pattern (deterministic <see cref="Dispose"/> plus a finalizer
/// backstop) so the Rust-side <c>Box</c> is always reclaimed exactly once.
/// </summary>
public abstract class NativeObject : IDisposable
{
    private IntPtr _handle;

    private protected NativeObject(IntPtr handle)
    {
        _handle = handle;
    }

    /// <summary>The raw handle; throws once disposed.</summary>
    internal IntPtr Handle =>
        _handle != IntPtr.Zero
            ? _handle
            : throw new ObjectDisposedException(GetType().Name);

    /// <summary>Type-specific native free function.</summary>
    private protected abstract void Free(IntPtr handle);

    public void Dispose()
    {
        DisposeCore();
        GC.SuppressFinalize(this);
    }

    private void DisposeCore()
    {
        IntPtr h = _handle;
        if (h != IntPtr.Zero)
        {
            _handle = IntPtr.Zero;
            Free(h);
        }
    }

    ~NativeObject() => DisposeCore();
}
