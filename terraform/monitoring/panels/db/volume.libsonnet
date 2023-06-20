local grafana   = import '../../grafonnet-lib/grafana.libsonnet';
local defaults  = import '../../grafonnet-lib/defaults.libsonnet';

local panels    = grafana.panels;
local targets   = grafana.targets;

local _configuration = defaults.configuration.timeseries
  .withUnit('decbytes')
  .addThreshold({
    color : 'red',
    value : 40000000000000, // 40TB, Max is 64TB.
  })
  .withSpanNulls(true);

{
  new(ds, vars)::
    panels.timeseries(
      title       = 'DocumentDB Volume',
      description = 'Max is 64TB',
      datasource  = ds.cloudwatch,
    )
    .configure(_configuration)

    .addTarget(targets.cloudwatch(
      datasource  = ds.cloudwatch,
      namespace   = 'AWS/DocDB',
      metricName  = 'VolumeBytesUsed',
      statistic   = 'Maximum',
      dimensions  = {
        DBClusterIdentifier: vars.docdb_cluster_id
      },
    ))
}
