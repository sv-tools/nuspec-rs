using System.Runtime.InteropServices;

// The .NET SDK copies runtimes/<rid>/native/ from the package, and the runtime probes the
// platform's naming (nuspec_test_lib.dll, libnuspec_test_lib.so, libnuspec_test_lib.dylib).
Native.hello();

static class Native
{
    [DllImport("nuspec_test_lib")]
    public static extern void hello();
}
