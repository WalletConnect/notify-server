local grafana   = import '../../grafonnet-lib/grafana.libsonnet';
local defaults  = import '../../grafonnet-lib/defaults.libsonnet';

local panels    = grafana.panels;
local targets   = grafana.targets;

local threshold = 100; // Reset to 0 after https://github.com/WalletConnect/notify-server/issues/374 is resolved

local _configuration = defaults.configuration.timeseries
  .withSoftLimit(
    axisSoftMin = 0,
    axisSoftMax = threshold * 1.2,
  )
  .withThresholdStyle(grafana.fieldConfig.thresholdStyle.Dashed)
  .addThreshold({
    color : defaults.values.colors.critical,
    value : threshold,
  });

local _alert(namespace, env, notifications) = grafana.alert.new(
  namespace     = namespace,
  name          = "%(env)s - 5XX alert"     % { env: grafana.utils.strings.capitalize(env) },
  message       = '%(env)s - Notify - 5XX alert'  % { env: grafana.utils.strings.capitalize(env) },
  notifications = notifications,
  noDataState   = 'no_data',
  period        = '0m',
  conditions    = [
    grafana.alertCondition.new(
      evaluatorParams = [ 50 ],
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
      title       = 'HTTP 5xx Rate',
      datasource  = ds.cloudwatch,
    )
    .configure(_configuration)
    .addPanelThreshold(
      op    = 'gt',
      value = threshold,
    )

    .setAlert(
      vars.environment,
      _alert(vars.namespace, vars.environment, vars.notifications)
    )

    .addTarget(targets.cloudwatch(
      alias       = 'ELB',
      datasource  = ds.cloudwatch,
      namespace   = 'AWS/ApplicationELB',
      metricName  = 'HTTPCode_ELB_5XX_Count',
      dimensions  = {
        LoadBalancer: vars.load_balancer
      },
      statistic   = 'Sum',
      refId       = 'ELB',
    ))
    .addTarget(targets.cloudwatch(
      alias       = 'Target',
      datasource  = ds.cloudwatch,
      namespace   = 'AWS/ApplicationELB',
      metricName  = 'HTTPCode_Target_5XX_Count',
      dimensions  = {
        LoadBalancer: vars.load_balancer
      },
      statistic   = 'Sum',
      refId       = 'Target',
    ))
}
