local grafana   = import '../../grafonnet-lib/grafana.libsonnet';
local defaults  = import '../../grafonnet-lib/defaults.libsonnet';

local panels    = grafana.panels;
local targets   = grafana.targets;

{
  new(ds, vars)::
    panels.timeseries(
      title       = 'Messages in processing state',
      datasource  = ds.prometheus,
    )
    .configure(defaults.configuration.timeseries)
    .addTarget(targets.prometheus(
      datasource  = ds.prometheus,
      expr        = 'sum(publishing_queue_processing_size{})',
      refId       = "pub_processing_size",
    ))
}
