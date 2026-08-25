param([string]$f)
$vn = "C:\Users\x\dev\varn\target\release\vn.exe"
$env:VN_OPT=1; $env:VN_OPT_TRACE=1
$fb = (& $vn debug -p bytecode $f 2>&1 | Select-String 'Unsupported\("' | ForEach-Object { if ($_ -match 'Unsupported\("(.+?)"\)'){$matches[1]} } | Sort-Object -Unique) -join '; '
Remove-Item Env:VN_OPT_TRACE
$opt = (& $vn run $f 2>&1 | Out-String).Trim()
Remove-Item Env:VN_OPT
$leg = (& $vn run $f 2>&1 | Out-String).Trim()
if ($opt -eq $leg) { $m = "MATCH" } else { $m = "DIFFER" }
$fbn = if ($fb) { $fb } else { "(none)" }
"{0}: {1} | fallback: {2}" -f (Split-Path $f -Leaf), $m, $fbn
if ($opt -ne $leg) { "--- VN_OPT ---"; $opt; "--- LEGACY ---"; $leg }
