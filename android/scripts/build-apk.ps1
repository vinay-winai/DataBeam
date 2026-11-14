param([string]$Sdk = 'X:\asproj', [string]$Studio = 'X:\as')
$ErrorActionPreference = 'Stop'
$androidRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$env:JAVA_HOME = Join-Path $Studio 'jbr'
$env:GRADLE_USER_HOME = Join-Path $androidRoot '.tools\gradle'
# A short local path avoids JBR Unix-domain socket failures on this PC.
$taskJavaOptions = $env:JAVA_TOOL_OPTIONS
$env:JAVA_TOOL_OPTIONS = "$taskJavaOptions -Djdk.net.unixdomain.tmpdir=$env:WINDIR\Temp -Djava.io.tmpdir=$env:WINDIR\Temp"
try {
    python (Join-Path $PSScriptRoot 'build.py') --sdk $Sdk
    if ($LASTEXITCODE -ne 0) { throw 'Native build failed' }
    & (Join-Path $androidRoot 'gradlew.bat') -p $androidRoot assembleDebug lintDebug --no-daemon --console=plain
    if ($LASTEXITCODE -ne 0) { throw 'Android build failed' }
} finally { $env:JAVA_TOOL_OPTIONS = $taskJavaOptions }
