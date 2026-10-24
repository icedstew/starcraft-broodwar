# Windows: rebuild agent/local/scratch/dump/bw116.c and bw116n.c with headless Ghidra.
#   powershell -File re\ghidra\dump.ps1 [-Ghidra <ghidra dir>] [-Install <1.16.1 dir>]
# analyzeHeadless.bat breaks on paths with spaces: run from a space-free path (a junction).
# Auto-analysis takes ~2 min; DumpAll (ParallelDecompiler, all cores) ~30 s for 4644 functions.
param(
    [string]$Ghidra = "$env:USERPROFILE\tools\ghidra_12.1.4_PUBLIC",
    [string]$Install = $env:SCBW_INSTALL
)
$ErrorActionPreference = 'Stop'
$Repo = (Resolve-Path "$PSScriptRoot\..\..").Path
if ($Repo -match ' ') { throw "repo path has a space ($Repo): use a junction" }
$S = "$Repo\agent\local\scratch"
New-Item -ItemType Directory -Force "$S\sc116", "$S\dump", "$S\ghidra_proj" | Out-Null
if (-not (Test-Path "$S\sc116\StarCraft.exe")) {
    Copy-Item "$Install\StarCraft.exe", "$Install\storm.dll" "$S\sc116"
}
$env:GHIDRA_HEADLESS_MAXMEM = '10G'
$H = "$Ghidra\support\analyzeHeadless.bat"
$cpu = [Environment]::ProcessorCount
& $H "$S\ghidra_proj" sc116 -import "$S\sc116\StarCraft.exe" -overwrite -max-cpu $cpu -log "$S\ghidra_proj\analyze.log"
Remove-Item "$S\dump\bw116.c" -ErrorAction SilentlyContinue
if (-not $env:DUMP_TIMEOUT) { $env:DUMP_TIMEOUT = '120' }
& $H "$S\ghidra_proj" sc116 -process StarCraft.exe -noanalysis -readOnly -max-cpu $cpu `
    -scriptPath "$PSScriptRoot" -postScript DumpAll.java "$S\dump\bw116.c" -log "$S\ghidra_proj\dump.log"
$Py = "$S\venv\Scripts\python.exe"
& $Py -I "$Repo\re\src_names.py" | Out-File -Encoding ascii "$S\dump\names_src.txt"
& $Py -I "$Repo\re\rename.py" --names ../agent/local/scratch/dump/names_src.txt "$S\dump\bw116.c" "$S\dump\bw116n.c"
