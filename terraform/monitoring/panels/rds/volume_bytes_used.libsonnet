local grafana   = import '../../grafonnet-lib/grafana.libsonnet';
local defaults  = import '../../grafonnet-lib/defaults.libsonnet';

local panels    = grafana.panels;
local targets   = grafana.targets;

{
  new(ds, vars)::
    panels.timeseries(
      title       = 'Volume Used',
      datasource  = ds.cloudwatch,
    )
    .configure(
      defaults.configuration.timeseries
      .withUnit(grafana.fieldConfig.units.DecBytes)
    )

    .addTarget(targets.cloudwatch(
      datasource    = ds.cloudwatch,
      namespace     = 'AWS/RDS',
      metricName    = 'VolumeBytesUsed',
      dimensions  = {
        DBClusterIdentifier: vars.rds_cluster_id,
      },
      matchExact  = true,
      statistic     = 'Average',
    ))

    .addTarget(targets.cloudwatch(
      datasource    = ds.cloudwatch,
      namespace     = 'AWS/RDS',
      metricName    = 'VolumeBytesUsed',
      dimensions  = {
        DBClusterIdentifier: vars.rds_cluster_id,
      },
      matchExact  = true,
      statistic     = 'Maximum',
    ))
}
