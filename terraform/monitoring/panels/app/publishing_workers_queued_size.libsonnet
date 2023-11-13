local grafana   = import '../../grafonnet-lib/grafana.libsonnet';
local defaults  = import '../../grafonnet-lib/defaults.libsonnet';

local panels    = grafana.panels;
local targets   = grafana.targets;

{
  new(ds, vars)::
    panels.timeseries(
      title       = 'Messages queue size',
      datasource  = ds.prometheus,
    )
    .configure(defaults.configuration.timeseries)
    .addTarget(targets.prometheus(
      datasource  = ds.prometheus,
      expr        = 'sum(publishing_queue_queued_size{})',
      refId       = "pub_msgs_queue_size",
    ))
}
