# Reference datasets

The datasets each command enriches from are large and are not bundled or
shipped. Each command takes its data path by flag (`--data`, or `--countries` /
`--municipalities` for `place`). The `nearest` command is the exception: its
reference data is a table you supply with `--to`, not a downloaded dataset.

## Download helper

`scripts/download_data.sh` fetches and unpacks any of the five sources into
`./data/`, one sub-directory per source, matching the example paths in the
command pages:

```bash
# GSHHG, GEBCO, Natural Earth, and GISCO need no details
scripts/download_data.sh download gshhg gebco countries lau

# the Marine Regions (IHO) download sits behind a short form
scripts/download_data.sh --mr-name "Your Name" \
  --mr-org "Your Institute" --mr-email you@example.org \
  --mr-country Norway download iho
```

Selected datasets download in parallel. Existing archives are kept (`--force`
re-downloads), and the multi-GB GEBCO grid resumes an interrupted download. Run
`scripts/download_data.sh --help` for all options.

Caveats baked into the script:

- The GEBCO grid is multi-GB; the download resumes if interrupted.
- The GISCO LAU bundle nests one zip per projection; only the EPSG 4326
  (lon/lat) layer is unpacked, since the commands expect lon/lat.
- The Marine Regions (IHO) download submits the site's statistics form on your
  behalf, so every field the form marks required has to be supplied:
  `--mr-name`, `--mr-org`, `--mr-email`, and `--mr-country`, plus
  `--mr-category` and `--mr-purpose`, which default to `academia` and
  `Research`. Those two are dropdowns on the form and accept only fixed values,
  so the script checks them before downloading anything and lists the valid ones
  if you miss:

  | Option | Accepted values |
  |--------|-----------------|
  | `--mr-category` | `academia`, `industry`, `government`, `civil society` |
  | `--mr-purpose` | `Conservation`, `Data exploration & testing`, `Education & workshops`, `Fisheries`, `Policy & Marine Spatial Planning`, `Mapping & visualisation`, `Maritime transport & cruise planning`, `Industry & offshore activities`, `Research`, `GIS Analysis`, `Personal information`, `Other` |

  Anything missing or invalid is prompted for at the terminal, with the two
  dropdowns offered as numbered menus, so you can also just run
  `scripts/download_data.sh download iho` and answer the questions. Prompting
  needs a terminal to ask at, so under `-y/--yes` or in a non-interactive shell
  (CI, a pipeline) a missing field stays a hard error rather than hanging.

  `--mr-country` is free text but has to match the form's spelling of the
  country in English. Because these details go to a third party and the download
  accepts the dataset licence (CC BY-NC-SA 4.0, non-commercial and share-alike),
  the script prints exactly what it will submit and waits for confirmation
  before starting. It also verifies the response is a zip and fails loudly if
  the form rejects the request.

## Sources

| Dataset | Used by | Source |
|---------|---------|--------|
| GSHHG shorelines (ESRI shapefiles, resolution `f`) | `coast` | <https://www.soest.hawaii.edu/pwessel/gshhg/> |
| GEBCO bathymetry (gridded NetCDF) | `depth` | <https://www.gebco.net/> |
| IHO Sea Areas v3 (GeoJSON or shapefile) | `sea` | <https://www.marineregions.org/> |
| Natural Earth countries | `place` | <https://www.naturalearthdata.com/> |
| Eurostat GISCO LAU (municipalities) | `place` | <https://ec.europa.eu/eurostat/web/gisco> |
