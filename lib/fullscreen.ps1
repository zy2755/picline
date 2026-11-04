# 前台窗口轮询:每 350ms 输出一行 JSON(矩形/样式/类名/标题),供全屏检测用
$ErrorActionPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

Add-Type -Namespace W -Name N -MemberDefinition '
[DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
[DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
[DllImport("user32.dll")] public static extern int GetWindowLongW(IntPtr h, int i);
[DllImport("user32.dll")] public static extern int GetClassNameW(IntPtr h, System.Text.StringBuilder s, int n);
[DllImport("user32.dll")] public static extern int GetWindowTextW(IntPtr h, System.Text.StringBuilder s, int n);
[StructLayout(LayoutKind.Sequential)] public struct RECT { public int L; public int T; public int R; public int B; }
'

$sb = New-Object System.Text.StringBuilder 512
while ($true) {
  $h = [W.N]::GetForegroundWindow()
  if ($h -ne [IntPtr]::Zero) {
    $r = New-Object W.N+RECT
    if ([W.N]::GetWindowRect($h, [ref]$r)) {
      $style = [W.N]::GetWindowLongW($h, -16)
      [void][W.N]::GetClassNameW($h, $sb, 512); $cls = $sb.ToString()
      $sb.Clear() | Out-Null
      [void][W.N]::GetWindowTextW($h, $sb, 512); $txt = $sb.ToString()
      $sb.Clear() | Out-Null
      ConvertTo-Json -Compress @{ l = $r.L; t = $r.T; r = $r.R; b = $r.B; st = $style; c = $cls; w = $txt }
    }
  }
  Start-Sleep -Milliseconds 350
}
