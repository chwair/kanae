# temporary: launches the instrumented build and tees its log for the seek diagnosis
$q = "D:\Documents\Kanae\6.8.0\msvc2022_64"
$env:PATH = "$q\bin;$env:PATH"
$env:QT_PLUGIN_PATH = "$q\plugins"
$env:QML_IMPORT_PATH = "$q\qml"
$env:QT_FORCE_STDERR_LOGGING = "1"
$log = "$env:TEMP\kanae-seek-diag.log"
Write-Host "logging to $log - play a track, seek a few times, then close the window"
& "D:\Documents\Kanae\target\debug\kanae.exe" -g 2>&1 | Tee-Object -FilePath $log
