local grafana   = import '../../grafonnet-lib/grafana.libsonnet';
local defaults  = import '../../grafonnet-lib/defaults.libsonnet';

local panels    = grafana.panels;
local targets   = grafana.targets;
local overrides = defaults.overrides;

{
  new(ds, vars)::
    panels.timeseries(
      title       = 'CPU Utilization',
      datasource  = ds.cloudwatch,
    )
    .configure(overrides.cpu(defaults.configuration.timeseries_resource))

    .setAlert(defaults.alerts.cpu(
      namespace     = vars.namespace,
      title         = 'ECS',
      env           = vars.environment,
      notifications = vars.notifications,
    ))

    .addTarget(targets.cloudwatch(
      alias       = "CPU (${PROP('Stat')})",
      datasource  = ds.cloudwatch,
      namespace   = 'AWS/ECS',
      metricName  = 'CPUUtilization',
      dimensions  = {
        ServiceName: vars.ecs_service_name
      },
      statistic   = 'Maximum',
      refId       = 'CPU_Max',
    ))
    .addTarget(targets.cloudwatch(
      alias       = "CPU (${PROP('Stat')})",
      datasource  = ds.cloudwatch,
      namespace   = 'AWS/ECS',
      metricName  = 'CPUUtilization',
      dimensions  = {
        ServiceName: vars.ecs_service_name
      },
      statistic   = 'Average',
      refId       = 'CPU_Avg',
    ))
}
