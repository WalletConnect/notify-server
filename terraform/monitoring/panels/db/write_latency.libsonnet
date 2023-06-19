local grafana   = import '../../grafonnet-lib/grafana.libsonnet';
local defaults  = import '../../grafonnet-lib/defaults.libsonnet';

local panels    = grafana.panels;
local targets   = grafana.targets;

local _configuration = defaults.configuration.timeseries
  .addThreshold({
    color : defaults.values.colors.warn,
    value : 0.015
  })
  .addThreshold({
    color : defaults.values.colors.critical,
    value : 0.5,
  })
  .withThresholdStyle(grafana.fieldConfig.thresholdStyle.area);


{
  new(ds, vars)::
    panels.timeseries(
      title       = 'Write Latency',
      datasource  = ds.cloudwatch,
    )
    .configure(_configuration)

    .addTarget(targets.cloudwatch(
      alias       = 'Write Latency',
      datasource  = ds.cloudwatch,
      namespace   = 'AWS/DocDB',
      dimensions  = {
        DBClusterIdentifier: vars.docdb_cluster_id
      },
      matchExact  = true,
      metricName  = 'WriteLatency',
      statistic   = 'Average',
      refId       = 'WriteLatency',
    )),
}
