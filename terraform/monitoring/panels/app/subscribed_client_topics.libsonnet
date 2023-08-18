local grafana   = import '../../grafonnet-lib/grafana.libsonnet';
local defaults  = import '../../grafonnet-lib/defaults.libsonnet';

local panels    = grafana.panels;
local targets   = grafana.targets;

{
  new(ds, vars)::
    panels.timeseries(
      title       = 'Subscribed Client Topics',
      datasource  = ds.prometheus,
    )
    .configure(defaults.configuration.timeseries)

    .addTarget(targets.prometheus(
      datasource    = ds.prometheus,
      expr          = 'sum(increase(subscribed_client_topics[$__rate_interval]))',
      legendFormat  = 'Subscribed Client Topics',
      exemplar      = true,
      refId       = 'Subscribed Client Topics',
    ))
}
