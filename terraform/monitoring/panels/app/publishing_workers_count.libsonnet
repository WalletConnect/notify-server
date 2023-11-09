local grafana   = import '../../grafonnet-lib/grafana.libsonnet';
local defaults  = import '../../grafonnet-lib/defaults.libsonnet';

local panels    = grafana.panels;
local targets   = grafana.targets;

{
  new(ds, vars)::
    panels.timeseries(
      title       = 'Publishing worker tasks count',
      datasource  = ds.prometheus,
    )
    .configure(defaults.configuration.timeseries)
    .addTarget(targets.prometheus(
      datasource  = ds.prometheus,
      expr        = 'sum(rate(spawned_publishing_workers{}[$__rate_interval]))',
      refId       = "availability",
    ))
}
