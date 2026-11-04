# StarDust KMDF region backing

Kernel-mode driver that owns named region pages and maps them into the calling process on `\\.\stardust`. Same ioctl contract as Linux `/dev/stardust`: CREATE or ATTACH, then MAP; no DESTROY.

Cargo never builds this tree. Userspace `Region::map_module` talks to the device; this host is not required to load the driver.

## Build (WDK, x64)

Requires Visual Studio with the Windows Driver Kit (WDK). Open a **Developer Command Prompt for VS** (or any shell where `msbuild` sees `WindowsKernelModeDriver10.0`).

From this directory:

```bat
msbuild stardust.vcxproj /p:Configuration=Release /p:Platform=x64
```

Debug:

```bat
msbuild stardust.vcxproj /p:Configuration=Debug /p:Platform=x64
```

Output is `x64\Release\stardust.sys` (or `x64\Debug\`) plus the INF in the package folder the WDK produces.

## Test-signing

Windows will not load an unsigned kernel driver.

1. Enable test-signing and reboot (Administrator):

   ```bat
   bcdedit /set testsigning on
   ```

   Disable Secure Boot in firmware if the OS refuses test-signing.

2. Make a test certificate (WDK `MakeCert` / PowerShell `New-SelfSignedCertificate`) and add it to the trusted root / trusted publisher stores on that machine.

3. Build a catalog and sign the `.sys` and `.cat`:

   ```bat
   inf2cat /driver:<packageDir> /os:10_X64
   signtool sign /v /fd SHA256 /s PrivateCertStore /n <TestCertName> stardust.sys
   signtool sign /v /fd SHA256 /s PrivateCertStore /n <TestCertName> stardust.cat
   ```

## Install / start (Administrator)

Install/start needs Administrator. After the device is running, `CreateFile` / `CREATE` / `MAP` do **not** (World RW, 0666 analog via `WdfDeviceInitAssignSDDLString` and INF `Security`).

From the package directory that contains `stardust.inf` and `stardust.sys`:

```bat
pnputil /add-driver stardust.inf /install
sc query stardust
sc start stardust
```

Root-enumerated hardware ID is `Root\Stardust`. With `devcon`:

```bat
devcon install stardust.inf Root\Stardust
```

Stop / remove (Administrator):

```bat
sc stop stardust
pnputil /delete-driver stardust.inf /uninstall /force
```

## Confirm `\\.\stardust`

After start, a non-admin process should be able to open the DOS device. PowerShell:

```powershell
$f = [System.IO.File]::Open('\\.\stardust', 'Open', 'ReadWrite', 'ReadWrite')
$f.Dispose()
'opened \\.\stardust'
```

Or:

```bat
fsutil file queryfileid \\.\stardust
```

Device Manager: **System devices** → **StarDust named region backing**.

If `CreateFile` fails with not-found, the service is not started. If it fails with access denied, the SDDL/INF Security did not apply — reinstall; do not run userspace as Administrator as the workaround.

Userspace: `Region::map_module` opens `\\.\stardust`, then `DeviceIoControl(CREATE)` and `DeviceIoControl(MAP)` on that handle. `Region::attach_module` opens the device, then `DeviceIoControl(ATTACH)` (0x802) and `DeviceIoControl(MAP)` on that handle. Do not `CreateFileMapping` on the device handle.
