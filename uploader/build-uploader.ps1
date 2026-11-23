# build-uploader.ps1 — build clé en main de l'uploader Windows pointé sur ton serveur.
#
# Usage (PowerShell, depuis la racine du repo storm-codex) :
#   .\uploader\build-uploader.ps1 -Server "http://<server-ip>:5102" -Token "<ton-token>"
#   # option installeur (si Inno Setup `iscc` est dans le PATH) :
#   .\uploader\build-uploader.ps1 -Server "http://<server-ip>:5102" -Token "<ton-token>" -Installer
#
# Ce qu'il fait : écrit un .env local à uploader/ (SERVER_URL + AUTH_TOKEN, gitignoré → reste local
# — PAS le .env racine du repo, qui est la config du serveur storm-codex lui-même),
# `cargo build --release` (l'URL et le token sont figés dans l'exe), et te donne le chemin.
# Au 1er lancement (box connecté) l'uploader backfill TOUTE ton archive, puis surveille les
# nouvelles parties. Token créable via : POST /api/admin/tokens (champ name) avec l'ADMIN_TOKEN du box.

param(
    [Parameter(Mandatory = $true)] [string] $Server,
    [Parameter(Mandatory = $true)] [string] $Token,
    [switch] $Installer
)

$ErrorActionPreference = "Stop"
# racine du repo = dossier parent de ce script
$RepoRoot = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$ClientDir = Join-Path $RepoRoot "uploader"

Write-Host "→ écriture du .env de uploader/ (SERVER_URL + AUTH_TOKEN, local/gitignoré)..." -ForegroundColor Cyan
@(
    "SERVER_URL=$Server"
    "AUTH_TOKEN=$Token"
) | Set-Content -Path (Join-Path $ClientDir ".env") -Encoding ASCII

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "cargo introuvable. Installe Rust : https://rustup.rs"
}

Write-Host "→ build release (URL + token figés dans le binaire)..." -ForegroundColor Cyan
# Capturé AVANT d'entrer dans le try/finally protecteur : Push-Location peut lui-même lever une
# exception (ex. $ClientDir disparu) après avoir déjà modifié la pile de localisation, ce qui la
# désynchroniserait du finally censé la restaurer. Set-Location directement, protégé dès le début.
$originalLocation = Get-Location
try {
    Set-Location -LiteralPath $ClientDir
    # Sauvegarde/restauration explicite : $env: modifie l'environnement du PROCESSUS PowerShell
    # appelant (ce n'est pas scopé au script, même invoqué sans dot-sourcing) — sans ça,
    # SERVER_URL/AUTH_TOKEN fuiraient dans la session de l'utilisateur après la fin du script, et
    # resteraient actifs pendant l'appel à iscc plus bas alors qu'ils n'ont aucune raison de l'être.
    # Imbriqué DANS le try/finally de localisation (pas en séquence) : si `cargo build` échoue et
    # lève une exception, le finally interne restaure l'env PUIS l'exception continue de se
    # propager vers le finally externe, qui restaure la localisation dans tous les cas.
    $prevServerUrl = $env:SERVER_URL
    $prevAuthToken = $env:AUTH_TOKEN
    try {
        # Passés aussi comme variables d'env (pas seulement via .env) : build.rs déclare
        # `cargo:rerun-if-env-changed=SERVER_URL/AUTH_TOKEN` sans condition, donc Cargo invalide
        # toujours correctement le cache sur un changement de valeur — contrairement au fichier
        # .env, dont le suivi (`rerun-if-changed`) n'est enregistré que s'il existait déjà lors
        # d'un build précédent. Sans ça, un premier `uploader/.env` créé après un build release
        # déjà en cache (ex. un simple `cargo build --release` de test, sans config) resterait
        # invisible à Cargo et produirait un exe non configuré sans aucune erreur.
        $env:SERVER_URL = $Server
        $env:AUTH_TOKEN = $Token
        cargo build --release
        if ($LASTEXITCODE -ne 0) { throw "cargo build --release a échoué (code $LASTEXITCODE)." }
    }
    finally {
        # Restaure l'état exact d'avant (valeur précédente, ou totalement absente).
        if ($null -eq $prevServerUrl) { Remove-Item Env:\SERVER_URL -ErrorAction SilentlyContinue } else { $env:SERVER_URL = $prevServerUrl }
        if ($null -eq $prevAuthToken) { Remove-Item Env:\AUTH_TOKEN -ErrorAction SilentlyContinue } else { $env:AUTH_TOKEN = $prevAuthToken }
    }

    $exe = Join-Path $ClientDir "target\release\hots-replay-client.exe"
    if (-not (Test-Path $exe)) { throw "Build terminé mais exe introuvable : $exe" }
    Write-Host "`n✔ Uploader prêt : $exe" -ForegroundColor Green
    Write-Host "  Lance-le (double-clic ou .\target\release\hots-replay-client.exe) : il détecte" -ForegroundColor Green
    Write-Host "  ton dossier de replays et backfill toute l'archive vers $Server." -ForegroundColor Green

    if ($Installer) {
        if (Get-Command iscc -ErrorAction SilentlyContinue) {
            Write-Host "`n→ build de l'installeur (Inno Setup)..." -ForegroundColor Cyan
            iscc (Join-Path $ClientDir "installer.iss")
            if ($LASTEXITCODE -ne 0) { throw "iscc a échoué (code $LASTEXITCODE)." }
            Write-Host "✔ Installeur dans uploader\installer-output\" -ForegroundColor Green
        } else {
            Write-Warning "Inno Setup (iscc) introuvable — installe-le (https://jrsoftware.org/isinfo.php) ou lance l'exe directement."
        }
    }
}
finally {
    Set-Location -LiteralPath $originalLocation
}
