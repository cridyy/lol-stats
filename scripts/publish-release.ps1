[CmdletBinding()]
param(
    [ValidateSet("All", "GitHub", "Gitee")]
    [string]$Target = "All",

    [switch]$SkipBuild,
    [switch]$SkipPush,
    [switch]$Prerelease,
    [switch]$ResetGiteeToken,
    [switch]$DryRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$manifestPath = Join-Path $repoRoot "update.json"
$giteeTokenPath = Join-Path ([Environment]::GetFolderPath("LocalApplicationData")) "lol-stats\release\gitee-token.txt"

function Invoke-Native {
    param(
        [Parameter(Mandatory)]
        [string]$FilePath,

        [Parameter(Mandatory)]
        [string[]]$Arguments
    )

    & $FilePath @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "命令执行失败（退出码 $LASTEXITCODE）：$FilePath $($Arguments -join ' ')"
    }
}

function Get-NativeOutput {
    param(
        [Parameter(Mandatory)]
        [string]$FilePath,

        [Parameter(Mandatory)]
        [string[]]$Arguments
    )

    $output = & $FilePath @Arguments 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "命令执行失败（退出码 $LASTEXITCODE）：$FilePath $($Arguments -join ' ')"
    }

    return ($output | Out-String).Trim()
}

function Get-HttpStatusCode {
    param([System.Exception]$Exception)

    if ($null -ne $Exception.Response -and $null -ne $Exception.Response.StatusCode) {
        return [int]$Exception.Response.StatusCode
    }

    return 0
}

function Get-RemoteRepository {
    param([Parameter(Mandatory)][string]$RemoteName)

    $remoteUrl = Get-NativeOutput -FilePath "git" -Arguments @("remote", "get-url", $RemoteName)
    $match = [regex]::Match(
        $remoteUrl,
        "^(?:https?://[^/]+/|git@[^:]+:)(?<owner>[^/]+)/(?<repo>[^/]+?)(?:\.git)?$"
    )
    if (-not $match.Success) {
        throw "无法解析远程仓库地址：$remoteUrl"
    }

    return [PSCustomObject]@{
        Owner = $match.Groups["owner"].Value
        Repo = $match.Groups["repo"].Value
    }
}

function Get-GitCredentialSecret {
    param([Parameter(Mandatory)][string]$HostName)

    $credentialInput = "protocol=https`nhost=$HostName`n`n"
    $credentialOutput = $credentialInput | & git credential fill 2>$null
    if ($LASTEXITCODE -ne 0) {
        return $null
    }

    $passwordLine = $credentialOutput | Where-Object { $_ -like "password=*" } | Select-Object -First 1
    if ([string]::IsNullOrWhiteSpace($passwordLine)) {
        return $null
    }

    return $passwordLine.Substring("password=".Length)
}

function Get-GitHubHeaders {
    param([Parameter(Mandatory)][string]$Token)

    return @{
        Authorization = "Bearer $Token"
        Accept = "application/vnd.github+json"
        "X-GitHub-Api-Version" = "2022-11-28"
        "User-Agent" = "lol-stats-release-script"
    }
}

function Get-GitHubToken {
    $token = $env:GH_TOKEN
    if ([string]::IsNullOrWhiteSpace($token)) {
        $token = $env:GITHUB_TOKEN
    }
    if ([string]::IsNullOrWhiteSpace($token)) {
        $token = Get-GitCredentialSecret -HostName "github.com"
    }
    if ([string]::IsNullOrWhiteSpace($token)) {
        throw "未找到 GitHub 凭据。请先完成 GitHub 网页授权登录或设置 GH_TOKEN。"
    }

    try {
        Invoke-RestMethod `
            -Method Get `
            -Uri "https://api.github.com/user" `
            -Headers (Get-GitHubHeaders -Token $token) | Out-Null
    }
    catch {
        throw "现有 GitHub 凭据不能创建 Release，请重新完成 GitHub 授权登录。"
    }

    return $token
}

function Test-GiteeToken {
    param([Parameter(Mandatory)][string]$Token)

    try {
        $encodedToken = [uri]::EscapeDataString($Token)
        Invoke-RestMethod -Method Get -Uri "https://gitee.com/api/v5/user?access_token=$encodedToken" | Out-Null
        return $true
    }
    catch {
        return $false
    }
}

function Save-GiteeToken {
    param([Parameter(Mandatory)][string]$Token)

    $directory = Split-Path -Parent $giteeTokenPath
    New-Item -ItemType Directory -Path $directory -Force | Out-Null
    $secureToken = ConvertTo-SecureString -String $Token -AsPlainText -Force
    $secureToken | ConvertFrom-SecureString | Set-Content -Path $giteeTokenPath -Encoding utf8
}

function Read-SavedGiteeToken {
    if (-not (Test-Path $giteeTokenPath)) {
        return $null
    }

    try {
        $encryptedToken = (Get-Content -Path $giteeTokenPath -Raw).Trim()
        $secureToken = $encryptedToken | ConvertTo-SecureString
        return [System.Net.NetworkCredential]::new("", $secureToken).Password
    }
    catch {
        return $null
    }
}

function Request-GiteeToken {
    Write-Host ""
    Write-Host "Gitee 创建 Release 需要私人令牌，仅首次配置。" -ForegroundColor Yellow
    Write-Host "创建地址：https://gitee.com/profile/personal_access_tokens" -ForegroundColor Cyan
    Write-Host "令牌至少需要 projects（仓库操作）权限。输入内容不会显示。" -ForegroundColor DarkGray

    $secureToken = Read-Host "请输入 Gitee 私人令牌" -AsSecureString
    $token = [System.Net.NetworkCredential]::new("", $secureToken).Password
    if ([string]::IsNullOrWhiteSpace($token) -or -not (Test-GiteeToken -Token $token)) {
        throw "Gitee 私人令牌无效，或缺少访问仓库所需的权限。"
    }

    Save-GiteeToken -Token $token
    Write-Host "Gitee 令牌已使用 Windows 当前用户加密保存。" -ForegroundColor Green
    return $token
}

function Get-GiteeToken {
    if ($ResetGiteeToken -and (Test-Path $giteeTokenPath)) {
        Remove-Item -LiteralPath $giteeTokenPath -Force
    }

    $candidates = @(
        $env:GITEE_TOKEN,
        (Read-SavedGiteeToken),
        (Get-GitCredentialSecret -HostName "gitee.com")
    )

    foreach ($candidate in $candidates) {
        if (-not [string]::IsNullOrWhiteSpace($candidate) -and (Test-GiteeToken -Token $candidate)) {
            return $candidate
        }
    }

    return Request-GiteeToken
}

function New-ReleaseBody {
    param(
        [Parameter(Mandatory)]$Manifest,
        [Parameter(Mandatory)][string]$AssetName,
        [Parameter(Mandatory)][string]$Sha256
    )

    $lines = @("## 更新内容", "")
    foreach ($note in $Manifest.notes) {
        $lines += "- $note"
    }

    $lines += @(
        "",
        "## 安装包",
        "",
        "- 文件：``$AssetName``",
        "- SHA256：``$Sha256``"
    )

    if (-not [string]::IsNullOrWhiteSpace([string]$Manifest.downloadUrl)) {
        $lines += @("", "## 备用下载", "", "- $($Manifest.downloadUrl)")
    }
    if (-not [string]::IsNullOrWhiteSpace([string]$Manifest.message)) {
        $lines += "- $($Manifest.message)"
    }

    return $lines -join "`n"
}

function Publish-GitHubRelease {
    param(
        [Parameter(Mandatory)][string]$Token,
        [Parameter(Mandatory)]$Repository,
        [Parameter(Mandatory)][string]$Tag,
        [Parameter(Mandatory)][string]$Title,
        [Parameter(Mandatory)][string]$Body,
        [Parameter(Mandatory)][string]$AssetPath
    )

    $headers = Get-GitHubHeaders -Token $Token
    $apiBase = "https://api.github.com/repos/$($Repository.Owner)/$($Repository.Repo)"
    $encodedTag = [uri]::EscapeDataString($Tag)
    $release = $null

    try {
        $release = Invoke-RestMethod -Method Get -Uri "$apiBase/releases/tags/$encodedTag" -Headers $headers
    }
    catch {
        if ((Get-HttpStatusCode -Exception $_.Exception) -ne 404) {
            throw
        }
    }

    $payload = @{
        tag_name = $Tag
        name = $Title
        body = $Body
        draft = $false
        prerelease = $Prerelease.IsPresent
    }

    if ($null -eq $release) {
        $release = Invoke-RestMethod `
            -Method Post `
            -Uri "$apiBase/releases" `
            -Headers $headers `
            -ContentType "application/json; charset=utf-8" `
            -Body ($payload | ConvertTo-Json -Depth 5)
        Write-Host "GitHub Release 已创建：$Tag" -ForegroundColor Green
    }
    else {
        $release = Invoke-RestMethod `
            -Method Patch `
            -Uri "$apiBase/releases/$($release.id)" `
            -Headers $headers `
            -ContentType "application/json; charset=utf-8" `
            -Body ($payload | ConvertTo-Json -Depth 5)
        Write-Host "GitHub Release 已更新：$Tag" -ForegroundColor Green
    }

    $assetName = [System.IO.Path]::GetFileName($AssetPath)
    $existingAsset = $release.assets | Where-Object { $_.name -eq $assetName } | Select-Object -First 1
    if ($null -ne $existingAsset) {
        Invoke-RestMethod -Method Delete -Uri "$apiBase/releases/assets/$($existingAsset.id)" -Headers $headers | Out-Null
    }

    $encodedAssetName = [uri]::EscapeDataString($assetName)
    $uploadUri = "https://uploads.github.com/repos/$($Repository.Owner)/$($Repository.Repo)/releases/$($release.id)/assets?name=$encodedAssetName"
    Invoke-RestMethod `
        -Method Post `
        -Uri $uploadUri `
        -Headers $headers `
        -ContentType "application/octet-stream" `
        -InFile $AssetPath | Out-Null

    Write-Host "GitHub 安装包上传完成：$assetName" -ForegroundColor Green
}

function Publish-GiteeRelease {
    param(
        [Parameter(Mandatory)][string]$Token,
        [Parameter(Mandatory)]$Repository,
        [Parameter(Mandatory)][string]$Tag,
        [Parameter(Mandatory)][string]$Title,
        [Parameter(Mandatory)][string]$Body,
        [Parameter(Mandatory)][string]$AssetPath
    )

    $apiBase = "https://gitee.com/api/v5/repos/$($Repository.Owner)/$($Repository.Repo)"
    $encodedToken = [uri]::EscapeDataString($Token)
    $encodedTag = [uri]::EscapeDataString($Tag)
    $release = $null

    try {
        $release = Invoke-RestMethod -Method Get -Uri "$apiBase/releases/tags/$encodedTag`?access_token=$encodedToken"
    }
    catch {
        if ((Get-HttpStatusCode -Exception $_.Exception) -ne 404) {
            throw
        }
    }

    $releaseForm = @{
        access_token = $Token
        tag_name = $Tag
        name = $Title
        body = $Body
        prerelease = $Prerelease.IsPresent
        target_commitish = "main"
    }

    if ($null -eq $release) {
        $release = Invoke-RestMethod -Method Post -Uri "$apiBase/releases" -Body $releaseForm
        Write-Host "Gitee Release 已创建：$Tag" -ForegroundColor Green
    }
    else {
        $release = Invoke-RestMethod -Method Patch -Uri "$apiBase/releases/$($release.id)" -Body $releaseForm
        Write-Host "Gitee Release 已更新：$Tag" -ForegroundColor Green
    }

    $attachmentsUri = "$apiBase/releases/$($release.id)/attach_files"
    $attachments = Invoke-RestMethod -Method Get -Uri "$attachmentsUri`?access_token=$encodedToken&per_page=100"
    $assetName = [System.IO.Path]::GetFileName($AssetPath)
    $existingAttachment = $attachments | Where-Object { $_.name -eq $assetName } | Select-Object -First 1
    if ($null -ne $existingAttachment) {
        Invoke-RestMethod `
            -Method Delete `
            -Uri "$attachmentsUri/$($existingAttachment.id)?access_token=$encodedToken" | Out-Null
    }

    $uploadedAttachment = Invoke-RestMethod `
        -Method Post `
        -Uri "$attachmentsUri`?access_token=$encodedToken" `
        -Form @{ file = (Get-Item -LiteralPath $AssetPath) }

    if ($null -eq $uploadedAttachment -or $uploadedAttachment.name -ne $assetName) {
        throw "Gitee 附件接口没有返回预期的安装包信息，上传结果无法确认。"
    }

    Write-Host "Gitee 安装包上传完成：$assetName" -ForegroundColor Green
}

Push-Location $repoRoot
try {
    if (-not (Test-Path $manifestPath)) {
        throw "缺少更新清单：$manifestPath"
    }

    $manifest = Get-Content -Path $manifestPath -Raw | ConvertFrom-Json
    $version = [string]$manifest.version
    if ($version -notmatch "^\d+\.\d+\.\d+$") {
        throw "update.json 中的版本号格式无效：$version"
    }

    $packageVersion = [string](Get-Content -Path "package.json" -Raw | ConvertFrom-Json).version
    $tauriVersion = [string](Get-Content -Path "src-tauri/tauri.conf.json" -Raw | ConvertFrom-Json).version
    $cargoText = Get-Content -Path "src-tauri/Cargo.toml" -Raw
    $cargoVersion = [regex]::Match($cargoText, '(?m)^version\s*=\s*"([^"]+)"').Groups[1].Value
    $versionMap = @{
        "package.json" = $packageVersion
        "tauri.conf.json" = $tauriVersion
        "Cargo.toml" = $cargoVersion
    }
    foreach ($entry in $versionMap.GetEnumerator()) {
        if ($entry.Value -ne $version) {
            throw "$($entry.Key) 的版本号为 $($entry.Value)，与 update.json 的 $version 不一致。"
        }
    }

    $assetPath = Join-Path $repoRoot "src-tauri\target\release\bundle\nsis\lol-stats_${version}_x64-setup.exe"
    $tag = "v$version"
    $title = if ([string]::IsNullOrWhiteSpace([string]$manifest.title)) { "LOL Stats $version" } else { [string]$manifest.title }

    Write-Host "准备发布 $title（$tag）" -ForegroundColor Cyan
    if ($DryRun) {
        Write-Host "DryRun 检查通过，不执行构建、推送或上传。" -ForegroundColor Yellow
        return
    }

    $worktreeStatus = Get-NativeOutput -FilePath "git" -Arguments @("status", "--porcelain")
    if (-not [string]::IsNullOrWhiteSpace($worktreeStatus)) {
        throw "工作区存在未提交改动。请先提交后再发布，避免 Release 与源码不一致。"
    }

    $branch = Get-NativeOutput -FilePath "git" -Arguments @("branch", "--show-current")
    if ([string]::IsNullOrWhiteSpace($branch)) {
        throw "当前处于 detached HEAD，无法自动推送发布。"
    }

    if (-not $SkipBuild) {
        Write-Host "开始构建 NSIS 安装包..." -ForegroundColor Cyan
        Invoke-Native -FilePath "npm" -Arguments @("run", "tauri", "build")
    }
    if (-not (Test-Path $assetPath)) {
        throw "没有找到安装包：$assetPath"
    }

    $headCommit = Get-NativeOutput -FilePath "git" -Arguments @("rev-parse", "HEAD")
    $existingTag = (& git tag --list $tag | Out-String).Trim()
    if ([string]::IsNullOrWhiteSpace($existingTag)) {
        Invoke-Native -FilePath "git" -Arguments @("tag", "-a", $tag, "-m", $title)
    }
    else {
        $tagCommit = Get-NativeOutput -FilePath "git" -Arguments @("rev-list", "-n", "1", $tag)
        if ($tagCommit -ne $headCommit) {
            throw "本地标签 $tag 已指向其他提交。请确认版本号或标签后再发布。"
        }
    }

    $publishGitHub = $Target -in @("All", "GitHub")
    $publishGitee = $Target -in @("All", "Gitee")
    if (-not $SkipPush) {
        if ($publishGitHub) {
            Invoke-Native -FilePath "git" -Arguments @("push", "github", $branch)
            Invoke-Native -FilePath "git" -Arguments @("push", "github", $tag)
        }
        if ($publishGitee) {
            Invoke-Native -FilePath "git" -Arguments @("push", "gitee", $branch)
            Invoke-Native -FilePath "git" -Arguments @("push", "gitee", $tag)
        }
    }

    $sha256 = (Get-FileHash -Path $assetPath -Algorithm SHA256).Hash.ToLowerInvariant()
    $releaseBody = New-ReleaseBody -Manifest $manifest -AssetName ([System.IO.Path]::GetFileName($assetPath)) -Sha256 $sha256

    if ($publishGitHub) {
        $githubToken = Get-GitHubToken
        $githubRepository = Get-RemoteRepository -RemoteName "github"
        Publish-GitHubRelease `
            -Token $githubToken `
            -Repository $githubRepository `
            -Tag $tag `
            -Title $title `
            -Body $releaseBody `
            -AssetPath $assetPath
    }

    if ($publishGitee) {
        $giteeToken = Get-GiteeToken
        $giteeRepository = Get-RemoteRepository -RemoteName "gitee"
        Publish-GiteeRelease `
            -Token $giteeToken `
            -Repository $giteeRepository `
            -Tag $tag `
            -Title $title `
            -Body $releaseBody `
            -AssetPath $assetPath
    }

    Write-Host ""
    Write-Host "发布完成：$title" -ForegroundColor Green
    Write-Host "安装包：$assetPath"
    Write-Host "SHA256：$sha256"
}
finally {
    Pop-Location
}
