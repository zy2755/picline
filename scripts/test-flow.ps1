# Interaction flow test (ASCII only): hover-reveal, click-copy, X-discard, auto-hide
Add-Type -TypeDefinition '
using System.Runtime.InteropServices;
public class DPI2 { [DllImport("user32.dll")] public static extern bool SetProcessDPIAware(); }
public class Mouse {
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint data, System.UIntPtr extra);
  public const uint DOWN = 0x0002, UP = 0x0004;
  public static void Click() { mouse_event(DOWN, 0, 0, 0, System.UIntPtr.Zero); System.Threading.Thread.Sleep(60); mouse_event(UP, 0, 0, 0, System.UIntPtr.Zero); }
}
'
[void][DPI2]::SetProcessDPIAware()

function Snap([string]$out) {
  Add-Type -AssemblyName System.Windows.Forms, System.Drawing
  $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
  $bmp = New-Object System.Drawing.Bitmap($b.Width, 300)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen(0, 0, 0, 0, (New-Object System.Drawing.Size($b.Width, 300)))
  $bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
  $g.Dispose(); $bmp.Dispose()
}

# 1) rest pointer on top edge center -> wait for reveal
[Mouse]::SetCursorPos(1280, 2)
Start-Sleep -Milliseconds 800
Snap "$env:TEMP\flow1-shown.png"
Write-Host "[1] hovered top edge, snapped"

# 2) move onto photo, single click -> copy to clipboard
[Mouse]::SetCursorPos(1280, 190)
Start-Sleep -Milliseconds 350
[Mouse]::Click()
Start-Sleep -Milliseconds 500
$img = Get-Clipboard -Format Image
if ($img) { Write-Host "[2] clipboard has image OK ($($img.Width)x$($img.Height))" } else { Write-Host "[2] clipboard EMPTY FAIL" }

# 3) click the x button -> peg falls into recycle bin
[Mouse]::SetCursorPos(1368, 80)
Start-Sleep -Milliseconds 250
[Mouse]::Click()
Start-Sleep -Milliseconds 800
Write-Host "[3] clicked X"

# 4) pointer away -> line should tuck away
[Mouse]::SetCursorPos(1280, 600)
Start-Sleep -Milliseconds 900
Snap "$env:TEMP\flow2-hidden.png"
Write-Host "[4] moved away, snapped"
