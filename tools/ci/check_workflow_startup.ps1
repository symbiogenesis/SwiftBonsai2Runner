#Requires -Version 7.6
<# Offline, fail-closed validation of the repository's generated v0.0.3 action lock subset.
   This does not validate arbitrary YAML or upstream provenance; gh actions-lock --verify owns that check. #>
[CmdletBinding()]
param([string]$Root = (Split-Path (Split-Path $PSScriptRoot -Parent) -Parent))
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Read-StartupScalar([string]$Value) {
    $Value = $Value.Trim()
    if ($Value -cmatch "^'((?:[^']|'')*)'\s*(?:#.*)?$") { return $Matches[1].Replace("''", "'") }
    if ($Value -cmatch '^("(?:[^"\\]|\\.)*")\s*(?:#.*)?$') { return ConvertFrom-Json $Matches[1] }
    if ($Value -cmatch '^[A-Za-z0-9_.$/@+\-]+(?:\s+#.*)?$') { return ($Value -split '\s+#', 2)[0] }
    throw 'expected a single simple string scalar (no flow mapping, anchor or multiline value)'
}
function Read-StartupReferences([string]$Source) {
    $blockIndent = -1; $number = 0
    foreach ($line in [IO.File]::ReadAllLines($Source,[Text.UTF8Encoding]::new($false,$true))) {
        $number++; $indent = $line.Length - $line.TrimStart(' ').Length
        if ($blockIndent -ge 0) { if (-not $line.Trim() -or $indent -gt $blockIndent) { continue }; $blockIndent = -1 }
        if ($line.Substring(0, [Math]::Min($indent + 1, $line.Length)).Contains("`t")) { throw "${Source}:${number}: YAML indentation must use spaces" }
        if ($line -cmatch '^\s*(?:-\s+)?(?:uses|''uses''|"uses"):\s*(.*)$') {
            try { $value = Read-StartupScalar $Matches[1] } catch { throw "${Source}:${number}: unsupported uses syntax: $_" }
            [pscustomobject]@{ Source=$Source; Line=$number; Value=$value }
        } elseif ($line -cmatch '^\s*(?:-\s*)?\{.*(?:\buses|[''"]uses[''"])\s*:') { throw "${Source}:${number}: write uses as a block mapping so the preflight can inspect it" }
        if ($line -cmatch '^\s*(?:-\s+)?(?:[\w-]+|''[^'']+''|"[^"]+"):\s*[|>][1-9+\-]*\s*(?:#.*)?$') { $blockIndent = $indent }
    }
}
function Read-StartupLock([string]$Source) {
    $workflows = [Collections.Generic.Dictionary[string,object]]::new([StringComparer]::Ordinal)
    $dependencies = [Collections.Generic.Dictionary[string,object]]::new([StringComparer]::Ordinal)
    $sections = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    $section = ''; $entry = ''; $version = ''; $number = 0
    foreach ($line in [IO.File]::ReadAllLines($Source,[Text.UTF8Encoding]::new($false,$true))) {
        $number++; if (-not $line.Trim() -or $line.TrimStart().StartsWith('#')) { continue }
        $indent = $line.Length - $line.TrimStart(' ').Length; $value = $line.Trim()
        try {
            if ($line.Contains("`t")) { throw 'tabs are outside the generated lockfile subset' }
            if ($indent -eq 0) {
                $entry = ''
                if ($value.StartsWith('version:') -and -not $version) { $version = Read-StartupScalar $value.Substring(8); $section = '' }
                elseif ($value -cin @('workflows:', 'dependencies:')) { $section = $value.TrimEnd(':'); if (-not $sections.Add($section)) { throw "duplicate $section section" } }
                else { throw 'unsupported or duplicate top-level field' }
            } elseif ($indent -eq 4 -and $section -cin @('workflows','dependencies')) {
                if ($value -cnotmatch '^(.+):(?:\s+(\[\]|\{\}))?$') { throw 'expected a generated workflow or dependency key' }
                $empty = $Matches[2]; $entry = Read-StartupScalar $Matches[1]
                $entries = if ($section -ceq 'workflows') { $workflows } else { $dependencies }
                if ($entries.ContainsKey($entry)) { throw "duplicate key $entry" }
                $expected = if ($section -ceq 'workflows') { '[]' } else { '{}' }
                if ($empty -and $empty -cne $expected) { throw 'wrong empty collection type' }
                if ($section -ceq 'workflows') { $entries[$entry] = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal) }
                else { $entries[$entry] = [Collections.Generic.Dictionary[string,object]]::new([StringComparer]::Ordinal) }
            } elseif ($indent -eq 8 -and $entry) {
                if ($section -ceq 'workflows' -and $value.StartsWith('- ')) {
                    $dependency = Read-StartupScalar $value.Substring(2)
                    if (-not $workflows[$entry].Add($dependency)) { throw "duplicate dependency $dependency" }
                } elseif ($section -ceq 'dependencies') {
                    $pair = $value -split ':',2; $key=$pair[0]
                    if ($pair.Count -ne 2 -or $key -cnotin @('ref','commit','owner_id','repo_id')) { throw 'unsupported dependency field' }
                    if ($dependencies[$entry].ContainsKey($key)) { throw "duplicate dependency field $key" }
                    if ($key -cin @('owner_id','repo_id')) {
                        if ($pair[1].Trim() -cnotmatch '^[1-9][0-9]*$') { throw "$key must be a positive integer" }; $field = [bigint]::Parse($pair[1].Trim())
                    } else { $field = Read-StartupScalar $pair[1] }
                    $dependencies[$entry][$key] = $field
                } else { throw 'expected a workflow dependency list item' }
            } else { throw 'unsupported generated lockfile structure or indentation' }
        } catch { throw "${Source}:${number}: $_" }
    }
    if ($version -cne 'v0.0.3' -or $sections.Count -ne 2 -or -not $sections.Contains('workflows') -or -not $sections.Contains('dependencies')) { throw "${Source}: expected complete generated lockfile schema v0.0.3" }
    foreach ($key in $dependencies.Keys) { if ($dependencies[$key].Count -ne 4 -or -not $dependencies[$key]['ref']) { throw "${Source}: incomplete dependency metadata for $key" } }
    return @{ Workflows=$workflows; Dependencies=$dependencies }
}
function Get-StartupExternalLockKey([string]$Reference) {
    # gh-actions-lock v0.4.1 serializes pins at owner/repository granularity:
    # internal/dep/dependency.go Key() and internal/lockfile/convertor.go depToPin().
    # Sub-action/reusable-workflow paths are traversal inputs, not lock key fields.
    # Validate the complete reference first and retain its full commit identity;
    # CanonicalRef/Pin.String only fold SHA spelling and owner/repository case.
    if ($Reference -cnotmatch '^(?<repository>[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)(?:/[A-Za-z0-9_.-]+)*@(?<sha>[0-9a-fA-F]{40})$') { throw 'expected a full 40-character commit SHA for the complete external reference' }
    return $Matches.repository.ToLowerInvariant() + '@' + $Matches.sha.ToLowerInvariant()
}
function Get-StartupClosure([string]$Source, $References, $Local, [string[]]$Ancestors = @()) {
    if ($Source -cin $Ancestors) { throw "${Source}: same-repository action dependency cycle" }
    foreach ($reference in $References[$Source]) {
        $key = "$Source`n$($reference.Line)"
        if ($Local.ContainsKey($key)) { Get-StartupClosure $Local[$key] $References $Local (@($Ancestors) + $Source) }
        elseif ($reference.Value -cmatch '^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+(?:/[A-Za-z0-9_.-]+)*@[0-9a-fA-F]{40}$') { Get-StartupExternalLockKey $reference.Value }
    }
}
function Test-WorkflowStartup([string]$RepositoryRoot) {
    $RepositoryRoot = [IO.Path]::GetFullPath($RepositoryRoot)
    $errors = [Collections.Generic.List[string]]::new(); $workflowDirectory = Join-Path $RepositoryRoot '.github/workflows'
    $workflows = @(Get-ChildItem -LiteralPath $workflowDirectory -File -ErrorAction SilentlyContinue | Where-Object Extension -cin @('.yml','.yaml') | Sort-Object FullName)
    if (-not $workflows.Count) { return ,@("${workflowDirectory}: no workflows found") }
    $pending = [Collections.Generic.Stack[string]]::new(); foreach ($path in $workflows) { $pending.Push($path.FullName) }
    foreach ($path in @(Get-ChildItem -LiteralPath (Join-Path $RepositoryRoot '.github/actions') -File -Recurse -ErrorAction SilentlyContinue | Where-Object Name -cin @('action.yml','action.yaml'))) { $pending.Push($path.FullName) }
    $references = @{}; $local = @{}
    while ($pending.Count) {
        $source = $pending.Pop(); if ($references.ContainsKey($source)) { continue }
        try { $references[$source] = @(Read-StartupReferences $source) } catch { $errors.Add($_.ToString()); $references[$source]=@() }
        foreach ($reference in $references[$source]) {
            $value = $reference.Value; $location = [IO.Path]::GetRelativePath($RepositoryRoot,$source).Replace('\','/') + ':' + $reference.Line
            if ($value.StartsWith('$/')) {
                $relative = $value.Substring(2); $target = [IO.Path]::GetFullPath((Join-Path $RepositoryRoot $relative))
                if ($value.Contains('@') -or $value.Contains('\') -or '..' -cin ($relative -split '/') -or [IO.Path]::IsPathRooted($relative) -or -not $target.StartsWith($RepositoryRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { $errors.Add("${location}: invalid same-repository action reference $value"); continue }
                $candidates = if (Test-Path -LiteralPath $target -PathType Container) { @('action.yml','action.yaml') | ForEach-Object { Join-Path $target $_ } | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } } elseif ((Test-Path -LiteralPath $target -PathType Leaf) -and [IO.Path]::GetExtension($target) -cin @('.yml','.yaml')) { $target }
                $candidates = @($candidates)
                if ($candidates.Count -ne 1) { $errors.Add("${location}: same-repository reference must identify exactly one action/workflow: $value"); continue }
                $local["$source`n$($reference.Line)"]=$candidates[0]; $pending.Push($candidates[0])
            } elseif ($value -cnotmatch '^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+(?:/[A-Za-z0-9_.-]+)*@[0-9a-fA-F]{40}$') { $errors.Add("${location}: use `$/ for same-repository actions or a full 40-character commit SHA for external actions: $value") }
        }
    }
    try { $lock = Read-StartupLock (Join-Path $workflowDirectory 'actions.lock') } catch { $errors.Add($_.ToString()); return ,$errors.ToArray() }
    $names = @($workflows | ForEach-Object { [IO.Path]::GetRelativePath($RepositoryRoot,$_.FullName).Replace('\','/') })
    foreach ($name in $names) { if (-not $lock.Workflows.ContainsKey($name)) { $errors.Add("actions.lock: missing workflow $name") } }
    foreach ($name in $lock.Workflows.Keys) { if ($name -cnotin $names) { $errors.Add("actions.lock: stale workflow $name") } }
    $all = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    foreach ($source in $workflows) {
        $name = [IO.Path]::GetRelativePath($RepositoryRoot,$source.FullName).Replace('\','/')
        try { $expected = @(Get-StartupClosure $source.FullName $references $local | Sort-Object -Unique) } catch { $errors.Add($_.ToString()); continue }
        $actual = if ($lock.Workflows.ContainsKey($name)) { @($lock.Workflows[$name]) } else { @() }
        foreach ($value in $expected) { [void]$all.Add($value); if ($value -cnotin $actual) { $errors.Add("${name}: lockfile missing exact uses key $value (matching commit metadata under a tag key is insufficient)") } }
        foreach ($value in $actual) { if ($value -cnotin $expected) { $errors.Add("${name}: lockfile has stale uses key $value") } }
    }
    foreach ($value in $all) {
        if (-not $lock.Dependencies.ContainsKey($value)) { $errors.Add("actions.lock: missing dependency metadata for exact uses key $value") }
        elseif ($lock.Dependencies[$value]['commit'] -cne ('sha1-' + ($value -split '@')[-1])) { $errors.Add("actions.lock: commit metadata disagrees with exact uses SHA for $value") }
    }
    foreach ($value in $lock.Dependencies.Keys) { if (-not $all.Contains($value)) { $errors.Add("actions.lock: stale dependency metadata for $value") } }
    return ,$errors.ToArray()
}
if ($MyInvocation.InvocationName -ne '.') {
    $failures = Test-WorkflowStartup $Root
    foreach ($failure in $failures) { [Console]::Error.WriteLine($failure) }
    if ($failures.Count) { exit 1 }
    'Workflow startup policy passed: exact action-lock coverage and full-SHA external references.'
}
