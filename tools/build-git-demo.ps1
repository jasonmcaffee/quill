# Builds a small git repository for exercising Unluminous's git integration by hand.
#
# It has three commits by three authors on three widely separated dates, so the blame column has a
# real spread of ages to colour; a branch to merge or rebase; a change that has not been committed,
# so the commit panel and the gutter's change bars have something to show; and an untracked file,
# so `Unversioned Files` is not empty.
#
# It is built OUTSIDE the repository, under the temporary folder, because `sample/` is a fixture a
# screenshot test counts the files in and a repository dropped into it would change that count.
#
#   pwsh tools/build-git-demo.ps1              -> %TEMP%\unluminous-git-demo
#   pwsh tools/build-git-demo.ps1 -At C:\some\where
#
# Then open it:  cargo run --release -- %TEMP%\unluminous-git-demo

param([string]$At = (Join-Path $env:TEMP 'unluminous-git-demo'))

$ErrorActionPreference = 'Stop'
$demo = $At
if (Test-Path $demo) { Remove-Item -Recurse -Force $demo }
New-Item -ItemType Directory -Path (Join-Path $demo 'src') | Out-Null
Push-Location $demo
try {
    git init -q --initial-branch=main
    git config user.name 'Jason'
    git config user.email 'jsinjasonjsin@gmail.com'
    git config commit.gpgsign false

    Set-Content -Encoding utf8 readme.md @'
# Git demo

A small project for exercising Unluminous's git integration by hand.
'@
    Set-Content -Encoding utf8 src/sqlClient.ts @'
import { createScopedSql } from '../db/sqlClient';
import { Injectable } from '@nestjs/common';

/** Lists messages in a chat in chronological order. */
export class MessageRepository {
  private sql: postgres.Sql;

  constructor() {
    this.sql = createScopedSql();
  }
}
'@
    git add -A
    $env:GIT_AUTHOR_DATE = '2026-01-14T09:00:00+00:00'
    git commit -q -m 'the first commit'

    Set-Content -Encoding utf8 src/version.ts "export const version = '0.1.0';"
    git add -A
    git -c user.name='Sam Okafor' -c user.email='sam@example.com' commit -q --date '2026-03-02T11:00:00+00:00' -m 'add a version'

    Add-Content -Encoding utf8 src/sqlClient.ts "`n/** Deletes every message in a chat. */`nasync deleteByChat(chatId: number) {}"
    git add -A
    git -c user.name='Kim Rivera' -c user.email='kim@example.com' commit -q --date '2026-07-21T16:00:00+00:00' -m 'add deleteByChat'

    git switch -q -c feature
    Set-Content -Encoding utf8 src/feature.ts 'export const feature = true;'
    git add -A
    git commit -q -m 'start a feature'
    git switch -q main

    Add-Content -Encoding utf8 src/version.ts '// a line that has not been committed'
    Set-Content -Encoding utf8 notes.txt 'scratch'
    Write-Host "Built $demo. Open it with: cargo run --release -- $demo"
}
finally { Pop-Location }
