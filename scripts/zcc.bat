@rem zig cc for armv7 musl (Station ABI); zig lives in ..\tools next to this scripts folder
@"%~dp0..\tools\zig-windows-x86_64-0.13.0\zig.exe" cc -target arm-linux-musleabihf -mcpu=cortex_a7 %*
