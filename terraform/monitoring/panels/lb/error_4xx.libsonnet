local grafana   = import '../../grafonnet-lib/grafana.libsonnet';
local defaults  = import '../../grafonnet-lib/defaults.libsonnet';

local panels    = grafana.panels;
local targets   = grafana.targets;

local threshold = 100;

local _configuration = defaults.configuration.timeseries
  .withSoftLimit(
    axisSoftMin = 0,
    axisSoftMax = threshold * 1.2,
  )
  .withThresholdStyle(grafana.fieldConfig.thresholdStyle.dashed)
  .addThreshold({
    color : defaults.values.colors.critical,
    value : threshold,
  });

local _alert(namespace, env, notifications) = grafana.alert.new(
  namespace     = namespace,
  name          = "%(env)s - 4XX alert"     % { env: grafana.utils.strings.capitalize(env) },
  message       = '%(env)s - Too many 4XX'  % { env: grafana.utils.strings.capitalize(env) },
  notifications = notifications,
  noDataState   = 'no_data',
  conditions    = [
    grafana.alertCondition.new(
      evaluatorParams = [ threshold ],
      evaluatorType   = 'gt',
      operatorType    = 'or',
      queryRefId      = 'ELB',
      queryTimeStart  = '5m',
      queryTimeEnd    = 'now',
      reducerType     = grafana.alert_reducers.Avg
    ),
    grafana.alertCondition.new(
      evaluatorParams = [ threshold ],
      evaluatorType   = 'gt',
      operatorType    = 'or',
      queryRefId      = 'Target',
      queryTimeStart  = '5m',
      queryTimeEnd    = 'now',
      reducerType     = grafana.alert_reducers.Avg
    ),
  ],
);

{
  new(ds, vars)::
    panels.timeseries(
      title       = '4XX',
      datasource  = ds.cloudwatch,
    )
    .configure(_configuration)
    .addPanelThreshold(
      op    = 'gt',
      value = threshold,
    )

    .setAlert(_alert(vars.namespace, vars.environment, vars.notifications))

    .addTarget(targets.cloudwatch(
      alias       = 'ELB',
      datasource  = ds.cloudwatch,
      namespace   = 'AWS/ApplicationELB',
      metricName  = 'HTTPCode_ELB_4XX_Count',
      dimensions  = {
        LoadBalancer: vars.load_balancer
      },
      matchExact  = true,
      statistic   = 'Sum',
      refId       = 'ELB',
    ))
    .addTarget(targets.cloudwatch(
      alias       = 'Target',
      datasource  = ds.cloudwatch,
      namespace   = 'AWS/ApplicationELB',
      metricName  = 'HTTPCode_Target_4XX_Count',
      dimensions  = {
        LoadBalancer: vars.load_balancer
      },
      matchExact  = true,
      statistic   = 'Sum',
      refId       = 'Target',
    ))
}
