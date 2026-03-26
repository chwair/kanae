param(
    [Parameter(Position = 0)][string]$Exe,
    [Parameter(ValueFromRemainingArguments)][string[]]$ExeArgs
)

$RepoRoot = Split-Path $PSScriptRoot -Parent
$env:PATH = "$RepoRoot\6.8.0\msvc2022_64\bin;$env:PATH"

& $Exe @ExeArgs
exit $LASTEXITCODE
