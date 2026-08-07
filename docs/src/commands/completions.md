# completions

Print a shell completion script, so Tab fills in subcommands, flags, and the
names `--region` accepts.

```bash
seastamp completions <SHELL>
```

Like [regions](./regions.md), this command takes no input table and enriches
nothing. It writes a script to stdout and exits. The script is generated from
the same `clap` command tree the parser uses, so it cannot drift out of step
with the real CLI.

Supported shells: `bash`, `zsh`, `fish`, `elvish`, `powershell`.

## Installing it

Write the script where your shell looks at startup, then start a new shell.

**bash**

```bash
mkdir -p ~/.local/share/bash-completion/completions
seastamp completions bash > ~/.local/share/bash-completion/completions/seastamp
```

That path is read automatically if the `bash-completion` package is installed.
Without it, put the script anywhere and source it from `~/.bashrc`:

```bash
seastamp completions bash > ~/.seastamp-completion.bash
echo 'source ~/.seastamp-completion.bash' >> ~/.bashrc
```

**zsh**

```bash
mkdir -p ~/.zfunc
seastamp completions zsh > ~/.zfunc/_seastamp
```

`~/.zfunc` has to be on `$fpath` before `compinit` runs, so `~/.zshrc` needs:

```bash
fpath=(~/.zfunc $fpath)
autoload -Uz compinit && compinit
```

**fish**

```bash
seastamp completions fish > ~/.config/fish/completions/seastamp.fish
```

**PowerShell**

```powershell
seastamp completions powershell | Out-String | Invoke-Expression
```

Append that line to your profile (`$PROFILE`) to make it stick.

## What completes

```
seastamp <TAB>                    coast  depth  sea  place  nearest  regions  completions
                                  (and the top-level flags, -h -V --help --version)
seastamp co<TAB>                  coast  completions
seastamp coast --<TAB>            every flag on coast, including the shared ones
seastamp coast --in-format <TAB>  auto  parquet  csv  tsv  csv.gz  tsv.gz
seastamp coast --region med<TAB>  mediterranean
seastamp completions <TAB>        bash  elvish  fish  powershell  zsh
```

Enumerated values (`--in-format`, `--out-format`, `--unit`) complete from the
same list the parser accepts. Path options complete as files, and `coast --data`
as a directory, since GSHHG is a directory of shapefiles.

`--region` completes from all 109 accepted names: `auto`, the seven presets, and
the 101 IHO Sea Areas. They are offered to the shell but kept out of `--help`,
where 109 values would bury every other flag. Run
[`seastamp regions`](./regions.md) to see them with their boxes.

## One Tab or two, in bash

By default bash completes on the first Tab only when exactly one candidate
matches, and needs a second Tab to list an ambiguous set. To get the list on the
first Tab, add this to `~/.inputrc`:

```
set show-all-if-ambiguous on
```

zsh and fish already list on the first Tab.

## Caveats

**A region name with a space stops at its first word, in bash and zsh.** Both
shells receive candidates as one space-separated list, so `--region Bar<TAB>`
completes to `Barentsz`, not `Barentsz Sea`. This is a limitation of the
generated scripts, not of seastamp. It is not silent: running the truncated name
reports the full one back at you.

```
$ seastamp coast cores.parquet --region Barentsz
Error: unknown region 'Barentsz'. Did you mean: Barentsz Sea?
```

Finish the name by hand, and quote it, as `--region "Barentsz Sea"`. fish
completes multi-word names whole.

**The script is a snapshot of one version.** It is generated when you run the
command, not read from the binary at completion time, so a script written by an
older seastamp will offer that version's flags. Regenerate after upgrading.

## Options

| Option | Default | Meaning |
|--------|---------|---------|
| `<SHELL>` | required | `bash`, `zsh`, `fish`, `elvish`, or `powershell` |

## See also

- [Installation](../installation.md) for getting the binary in the first place
- [regions](./regions.md) for the names `--region` completes to
- [Regions](../reference/regions.md) for what a region does
